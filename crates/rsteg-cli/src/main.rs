//! `rsteg` — command-line steganography tool.
//!
//! Scope in this PR: `embed`, `extract`, `inspect`, `list`, `version`, `help`
//! verbs covering BMP/WAV/PNG with linear + permuted schemes and optional
//! XChaCha20-Argon2id encryption. See `specs/03-cli.md`.
//!
//! We hand-parse args with `lexopt` (no `clap`, no proc macros — spec 02).

#![deny(unsafe_code)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{
    CryptoScheme, Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader,
    SchemeFourcc,
};
use rsteg_crypto_aead::XChaCha20Argon2id;
use rsteg_png::PNG_ADAPTER;
use rsteg_wav::WAV_ADAPTER;

/// Exit codes per spec 03 §"Exit codes".
const EXIT_OK: u8 = 0;
const EXIT_GENERIC: u8 = 1;
const EXIT_ARGS: u8 = 2;
const EXIT_FORMAT: u8 = 3;
const EXIT_CAPACITY: u8 = 4;
const EXIT_EXTRACT: u8 = 5;
const EXIT_PASSPHRASE: u8 = 6;
const EXIT_IO: u8 = 7;
const EXIT_POLICY: u8 = 8;

fn main() -> ExitCode {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let verb = args.first().cloned().unwrap_or_default();
    let rest = if args.is_empty() { Vec::new() } else { args.split_off(1) };

    let verb = verb.to_string_lossy();
    let code = match verb.as_ref() {
        "" | "-h" | "--help" | "help" => {
            print_top_help();
            EXIT_OK
        }
        "-V" | "--version" | "version" => {
            print_version();
            EXIT_OK
        }
        "embed" => run_embed(rest),
        "extract" => run_extract(rest),
        "inspect" => run_inspect(rest),
        "list" => run_list(rest),
        other => {
            eprintln!("error: unknown verb '{other}'. Try `rsteg --help`.");
            EXIT_ARGS
        }
    };
    ExitCode::from(code)
}

fn print_version() {
    println!("rsteg {}", env!("CARGO_PKG_VERSION"));
    println!("features: bmp wav png crypto-aead");
}

fn print_top_help() {
    println!(
        r#"rsteg — steganography CLI

USAGE:
    rsteg <VERB> [OPTIONS]

VERBS:
    embed       Hide a payload inside a carrier file
    extract     Recover a payload from a stego file
    inspect     Report format + capacity for a file
    list        List compiled-in formats and schemes
    version     Print version and enabled features
    help        This message

Try `rsteg <verb> --help` for verb-specific options."#
    );
}

// ---------------- embed ----------------

#[derive(Default, Debug)]
struct EmbedArgs {
    input: Option<PathBuf>,
    payload: Option<PathBuf>,
    payload_text: Option<String>,
    output: Option<PathBuf>,
    password: Option<PasswordSpec>,
    format: Option<String>,
    scheme: Option<String>,
    density: u8,
    force: bool,
    allow_aggressive_density: bool,
}

#[derive(Debug, Clone)]
enum PasswordSpec {
    Stdin,
    Env(String),
    File(PathBuf),
    InsecureArg(String),
}

fn run_embed(args: Vec<std::ffi::OsString>) -> u8 {
    let parsed = match parse_embed_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ARGS;
        }
    };

    let input = match parsed.input.as_ref() {
        Some(p) => p,
        None => {
            eprintln!("error: --in is required");
            return EXIT_ARGS;
        }
    };
    let output = match parsed.output.as_ref() {
        Some(p) => p,
        None => {
            eprintln!("error: --out is required");
            return EXIT_ARGS;
        }
    };

    let carrier = match read_file(input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: read {:?}: {e}", input);
            return EXIT_IO;
        }
    };

    let payload: Vec<u8> = match (&parsed.payload, &parsed.payload_text) {
        (Some(_), Some(_)) => {
            eprintln!("error: --payload and --payload-text are mutually exclusive");
            return EXIT_ARGS;
        }
        (Some(p), None) => match read_file_or_stdin(p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("error: read payload {p:?}: {e}");
                return EXIT_IO;
            }
        },
        (None, Some(t)) => t.clone().into_bytes(),
        (None, None) => {
            eprintln!("error: --payload or --payload-text is required");
            return EXIT_ARGS;
        }
    };

    let (adapter, fmt_id) = match select_adapter(&carrier, parsed.format.as_deref()) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    let density = match parsed.density {
        0 | 1 => Density::Low,
        2 => Density::Moderate,
        3 | 4 if !parsed.allow_aggressive_density => {
            eprintln!(
                "error: density {} requires --allow-aggressive-density",
                parsed.density
            );
            return EXIT_POLICY;
        }
        3 => Density::Aggressive3,
        4 => Density::Aggressive4,
        d => {
            eprintln!("error: density {d} is outside 1..=4");
            return EXIT_ARGS;
        }
    };

    let scheme_id = parsed.scheme.as_deref().unwrap_or_else(|| {
        if parsed.password.is_some() {
            default_permuted_scheme(fmt_id)
        } else {
            default_linear_scheme(fmt_id)
        }
    });
    let scheme_fourcc = scheme_fourcc_for(scheme_id);
    let is_permuted = scheme_id.ends_with("-permuted");
    let is_encrypted = parsed.password.is_some();

    let password_bytes: Option<Vec<u8>> = match parsed.password.as_ref() {
        None => None,
        Some(spec) => match read_passphrase(spec) {
            Ok(b) => Some(b),
            Err(e) => {
                eprintln!("error: passphrase: {e}");
                return EXIT_PASSPHRASE;
            }
        },
    };

    // Derive seed for permuted schemes from the passphrase if present; else
    // a zero seed (deterministic, not secret — a no-password permuted mode
    // isn't secrecy, only obfuscation). When crypto + permuted are both on,
    // we use the passphrase for both the seed and the AEAD key.
    let seed: Option<u64> = if is_permuted {
        let pw = password_bytes.as_deref().unwrap_or(&[]);
        Some(fold_u64(pw))
    } else {
        None
    };

    let embed_opts = EmbedOpts {
        scheme: Some(static_str(scheme_id)),
        density,
        seed,
    };

    // Build the framed body:
    //   - encrypted: body = AEAD ciphertext of payload
    //   - plaintext: body = payload
    let crypto_fourcc = if is_encrypted {
        XChaCha20Argon2id::new().fourcc()
    } else {
        [0; 4]
    };
    // AEAD ciphertext is 58 bytes longer than plaintext (42-byte inner crypto
    // header + 16-byte Poly1305 tag). Pre-compute so the header we sign as
    // AAD matches the header written to disk (body_len included).
    const AEAD_OVERHEAD: u32 = 58;
    let (header, body): (PayloadHeader, Vec<u8>) = if is_encrypted {
        let mut h = PayloadHeader::plain(scheme_fourcc, density, &[]);
        h.flags |= PayloadHeader::FLAG_ENCRYPTED;
        if is_permuted {
            h.flags |= PayloadHeader::FLAG_PERMUTED;
        }
        h.crypto_fourcc = crypto_fourcc;
        h.body_len = payload.len() as u32 + AEAD_OVERHEAD;
        h.body_crc32 = 0;

        let aad = h.encode();
        let pw = password_bytes.as_deref().unwrap_or(&[]);
        let scheme = XChaCha20Argon2id::new();
        let ct = match scheme.seal(&payload, pw, &aad) {
            Ok(ct) => ct,
            Err(e) => {
                eprintln!("error: encrypt: {e}");
                return EXIT_GENERIC;
            }
        };
        debug_assert_eq!(ct.len() as u32, h.body_len);
        (h, ct)
    } else {
        let mut h = PayloadHeader::plain(scheme_fourcc, density, &payload);
        if is_permuted {
            h.flags |= PayloadHeader::FLAG_PERMUTED;
        }
        (h, payload.clone())
    };
    let framed = header.encode_with(&body);

    if output_exists_and_not_forced(output, parsed.force) {
        eprintln!(
            "error: --out {:?} already exists; pass --force to overwrite",
            output
        );
        return EXIT_POLICY;
    }

    let stego = match adapter.embed(&carrier, &framed, &embed_opts) {
        Ok(b) => b,
        Err(e) => return format_error_exit(&e),
    };

    if let Err(e) = write_file_or_stdout(output, &stego) {
        eprintln!("error: write {:?}: {e}", output);
        return EXIT_IO;
    }

    eprintln!(
        "embedded {}B into {}B carrier → {}B stego ({} bytes framed)",
        payload.len(),
        carrier.len(),
        stego.len(),
        framed.len()
    );
    EXIT_OK
}

fn parse_embed_args(args: Vec<std::ffi::OsString>) -> Result<EmbedArgs, String> {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_args(args);
    let mut out = EmbedArgs {
        density: 1,
        ..EmbedArgs::default()
    };
    while let Some(arg) = parser.next().map_err(|e| e.to_string())? {
        match arg {
            Short('i') | Long("in") => out.input = Some(parser.value().map_err(|e| e.to_string())?.into()),
            Short('p') | Long("payload") => out.payload = Some(parser.value().map_err(|e| e.to_string())?.into()),
            Long("payload-text") => {
                out.payload_text = Some(parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?);
            }
            Short('o') | Long("out") => out.output = Some(parser.value().map_err(|e| e.to_string())?.into()),
            Long("password") => {
                let v = parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "password spec must be utf-8")?;
                out.password = Some(parse_password_spec(&v)?);
            }
            Long("password-insecure-arg") => {
                let v = parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?;
                out.password = Some(PasswordSpec::InsecureArg(v));
                eprintln!("warning: --password-insecure-arg reveals the passphrase in argv");
            }
            Long("format") => out.format = Some(parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?),
            Long("scheme") => out.scheme = Some(parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?),
            Long("density") => {
                let v = parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?;
                out.density = v.parse().map_err(|_| "density must be 1..=4")?;
            }
            Long("allow-aggressive-density") => out.allow_aggressive_density = true,
            Long("force") => out.force = true,
            Short('h') | Long("help") => {
                print_embed_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unexpected arg: {:?}", arg)),
        }
    }
    Ok(out)
}

fn print_embed_help() {
    println!(
        r#"rsteg embed — hide a payload

USAGE:
    rsteg embed --in COVER (--payload FILE | --payload-text STR) --out STEGO [--password SPEC]

OPTIONS:
    -i, --in PATH              Cover file (required)
    -p, --payload PATH         Payload file (-, for stdin)
        --payload-text STR     Inline payload string
    -o, --out PATH             Output stego file (-, for stdout)
        --password SPEC        Passphrase source: -, env:VAR, file:PATH
        --password-insecure-arg STR
                               Pass literal passphrase (not recommended)
        --format FMT           Override detection: bmp|wav|png
        --scheme NAME          Override scheme
        --density N            1 (default) or 2; 3/4 require --allow-aggressive-density
        --allow-aggressive-density
        --force                Overwrite --out if it exists
    -h, --help                 This help"#
    );
}

// ---------------- extract ----------------

#[derive(Default, Debug)]
struct ExtractArgs {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    password: Option<PasswordSpec>,
    format: Option<String>,
    scheme: Option<String>,
    force: bool,
}

fn run_extract(args: Vec<std::ffi::OsString>) -> u8 {
    let parsed = match parse_extract_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_ARGS;
        }
    };

    let input = match parsed.input.as_ref() {
        Some(p) => p,
        None => {
            eprintln!("error: --in is required");
            return EXIT_ARGS;
        }
    };
    let output = match parsed.output.as_ref() {
        Some(p) => p,
        None => {
            eprintln!("error: --out is required");
            return EXIT_ARGS;
        }
    };
    let stego = match read_file(input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: read {:?}: {e}", input);
            return EXIT_IO;
        }
    };

    let (adapter, _fmt_id) = match select_adapter(&stego, parsed.format.as_deref()) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let password_bytes: Option<Vec<u8>> = match parsed.password.as_ref() {
        None => None,
        Some(spec) => match read_passphrase(spec) {
            Ok(b) => Some(b),
            Err(e) => {
                eprintln!("error: passphrase: {e}");
                return EXIT_PASSPHRASE;
            }
        },
    };

    // Scheme: if caller didn't specify, probe linear first; if that doesn't
    // find a header, fall back to permuted with seed derived from the
    // passphrase (if any). Simple heuristic — the spec-full "detector"
    // pipeline is phase 1.5+.
    let schemes_to_try: Vec<&'static str> = match parsed.scheme.as_deref() {
        Some(s) => vec![static_str(s)],
        None => {
            let (linear, permuted) = default_schemes_for(adapter.id());
            vec![linear, permuted]
        }
    };

    for scheme_id in schemes_to_try {
        let seed = if scheme_id.ends_with("-permuted") {
            Some(fold_u64(password_bytes.as_deref().unwrap_or(&[])))
        } else {
            None
        };
        let opts = ExtractOpts {
            scheme: Some(scheme_id),
            density: None,
            skip_header: false,
            raw_bit_count: None,
            seed,
        };
        let framed = match adapter.extract(&stego, &opts) {
            Ok(b) => b,
            Err(_) => continue,
        };
        return finalize_extract(framed, output, parsed.force, password_bytes.as_deref());
    }

    eprintln!("error: no rsteg payload found in {:?}", input);
    EXIT_EXTRACT
}

fn finalize_extract(
    framed: Vec<u8>,
    output: &PathBuf,
    force: bool,
    password: Option<&[u8]>,
) -> u8 {
    let header = match PayloadHeader::decode(&framed) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: header decode: {e}");
            return EXIT_EXTRACT;
        }
    };
    let encrypted = (header.flags & PayloadHeader::FLAG_ENCRYPTED) != 0;

    match (encrypted, password) {
        (true, None) => {
            eprintln!("error: file is encrypted; --password is required");
            return EXIT_PASSPHRASE;
        }
        (false, Some(_)) => {
            eprintln!("error: file is plaintext; omit --password");
            return EXIT_PASSPHRASE;
        }
        _ => {}
    }

    let body = &framed[PayloadHeader::SIZE..];
    let payload: Vec<u8> = if encrypted {
        // AAD is the header as written (with body_len = ciphertext length).
        let aad = header.encode();
        let scheme = XChaCha20Argon2id::new();
        match scheme.open(body, password.unwrap(), &aad) {
            Ok(pt) => pt,
            Err(e) => {
                eprintln!("error: decrypt: {e}");
                return EXIT_PASSPHRASE;
            }
        }
    } else {
        body.to_vec()
    };

    if output_exists_and_not_forced(output, force) {
        eprintln!(
            "error: --out {:?} already exists; pass --force to overwrite",
            output
        );
        return EXIT_POLICY;
    }

    if let Err(e) = write_file_or_stdout(output, &payload) {
        eprintln!("error: write {:?}: {e}", output);
        return EXIT_IO;
    }

    eprintln!("extracted {}B", payload.len());
    EXIT_OK
}

fn parse_extract_args(args: Vec<std::ffi::OsString>) -> Result<ExtractArgs, String> {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_args(args);
    let mut out = ExtractArgs::default();
    while let Some(arg) = parser.next().map_err(|e| e.to_string())? {
        match arg {
            Short('i') | Long("in") => out.input = Some(parser.value().map_err(|e| e.to_string())?.into()),
            Short('o') | Long("out") => out.output = Some(parser.value().map_err(|e| e.to_string())?.into()),
            Long("password") => {
                let v = parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "password spec must be utf-8")?;
                out.password = Some(parse_password_spec(&v)?);
            }
            Long("password-insecure-arg") => {
                let v = parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?;
                out.password = Some(PasswordSpec::InsecureArg(v));
            }
            Long("format") => out.format = Some(parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?),
            Long("scheme") => out.scheme = Some(parser.value().map_err(|e| e.to_string())?.into_string().map_err(|_| "invalid utf8")?),
            Long("force") => out.force = true,
            Short('h') | Long("help") => {
                println!("rsteg extract --in FILE --out FILE [--password SPEC]");
                std::process::exit(0);
            }
            _ => return Err(format!("unexpected arg: {:?}", arg)),
        }
    }
    Ok(out)
}

// ---------------- inspect ----------------

fn run_inspect(args: Vec<std::ffi::OsString>) -> u8 {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_args(args);
    let mut input: Option<PathBuf> = None;
    while let Some(arg) = parser.next().unwrap_or(None) {
        match arg {
            Short('i') | Long("in") => input = Some(parser.value().unwrap().into()),
            _ => {}
        }
    }
    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("usage: rsteg inspect --in FILE");
            return EXIT_ARGS;
        }
    };
    let bytes = match read_file(&input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {e}");
            return EXIT_IO;
        }
    };
    let (adapter, id) = match select_adapter(&bytes, None) {
        Ok(p) => p,
        Err(code) => return code,
    };
    println!("format: {id} ({} bytes)", bytes.len());

    // Try linear extract as a probe.
    let probe = adapter.extract(
        &bytes,
        &ExtractOpts {
            scheme: Some(static_str(default_linear_scheme(id))),
            density: None,
            skip_header: false,
            raw_bit_count: None,
            seed: None,
        },
    );
    match probe {
        Ok(f) => {
            if let Ok(h) = PayloadHeader::decode(&f) {
                println!(
                    "stego: scheme_fourcc={:?} density={} encrypted={} permuted={} body_len={}",
                    std::str::from_utf8(&h.scheme_fourcc.0).unwrap_or("????"),
                    h.density,
                    (h.flags & PayloadHeader::FLAG_ENCRYPTED) != 0,
                    (h.flags & PayloadHeader::FLAG_PERMUTED) != 0,
                    h.body_len
                );
            } else {
                println!("stego: linear probe found no rsteg header");
            }
        }
        Err(e) => {
            println!("stego: linear probe failed: {e}");
        }
    }
    EXIT_OK
}

// ---------------- list ----------------

fn run_list(_args: Vec<std::ffi::OsString>) -> u8 {
    println!("formats:");
    println!("  bmp  — 24-bit uncompressed (BI_RGB)");
    println!("  wav  — RIFF/PCM, 8-bit unsigned or 16-bit signed LE");
    println!("  png  — RGB8 / RGBA8, filters 0-4, no Adam7");
    println!("schemes:");
    println!("  bmp-lsb-linear    bmp-lsb-permuted");
    println!("  wav-lsb-linear    wav-lsb-permuted");
    println!("  png-lsb-linear    (permuted: pending phase 1)");
    println!("crypto:");
    println!("  xchacha20-argon2id  fourcc=XCA1 (default when --password)");
    EXIT_OK
}

// ---------------- helpers ----------------

fn select_adapter(bytes: &[u8], forced: Option<&str>) -> Result<(&'static dyn FormatAdapter, &'static str), u8> {
    if let Some(id) = forced {
        return Ok(adapter_by_id(id)).and_then(|opt| {
            opt.map(|a| (a, adapter_static_id(a.id())))
                .ok_or_else(|| {
                    eprintln!("error: unknown format '{id}'");
                    EXIT_ARGS
                })
        });
    }
    // Probe each adapter in order.
    if BMP_ADAPTER.recognize(bytes) {
        return Ok((&BMP_ADAPTER, "bmp"));
    }
    if WAV_ADAPTER.recognize(bytes) {
        return Ok((&WAV_ADAPTER, "wav"));
    }
    if PNG_ADAPTER.recognize(bytes) {
        return Ok((&PNG_ADAPTER, "png"));
    }
    eprintln!("error: could not detect format; try --format FMT");
    Err(EXIT_FORMAT)
}

fn adapter_by_id(id: &str) -> Option<&'static dyn FormatAdapter> {
    match id {
        "bmp" => Some(&BMP_ADAPTER),
        "wav" => Some(&WAV_ADAPTER),
        "png" => Some(&PNG_ADAPTER),
        _ => None,
    }
}

fn adapter_static_id(id: &'static str) -> &'static str {
    id
}

fn default_linear_scheme(fmt: &str) -> &'static str {
    match fmt {
        "bmp" => "bmp-lsb-linear",
        "wav" => "wav-lsb-linear",
        "png" => "png-lsb-linear",
        _ => "bmp-lsb-linear",
    }
}

fn default_permuted_scheme(fmt: &str) -> &'static str {
    match fmt {
        "bmp" => "bmp-lsb-permuted",
        "wav" => "wav-lsb-permuted",
        "png" => "png-lsb-linear", // png permuted not in phase 1 yet
        _ => "bmp-lsb-permuted",
    }
}

fn default_schemes_for(fmt: &str) -> (&'static str, &'static str) {
    (default_linear_scheme(fmt), default_permuted_scheme(fmt))
}

fn scheme_fourcc_for(id: &str) -> SchemeFourcc {
    match id {
        "bmp-lsb-linear" => SchemeFourcc::BMP_LSB_LINEAR,
        "bmp-lsb-permuted" => SchemeFourcc::BMP_LSB_PERMUTED,
        "wav-lsb-linear" => SchemeFourcc::WAV_LSB_LINEAR,
        "wav-lsb-permuted" => SchemeFourcc::WAV_LSB_PERMUTED,
        "png-lsb-linear" => SchemeFourcc::PNG_LSB_LINEAR,
        "png-lsb-permuted" => SchemeFourcc::PNG_LSB_PERMUTED,
        _ => SchemeFourcc::ZERO,
    }
}

/// Very small SplitMix64-esque passphrase-to-seed fold. Deterministic per
/// passphrase. Not cryptographic — permuted scheme gets confidentiality
/// from AEAD, not from the seed being secret.
fn fold_u64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

fn read_file(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    if path == &PathBuf::from("-") {
        let mut v = Vec::new();
        std::io::stdin().read_to_end(&mut v)?;
        Ok(v)
    } else {
        std::fs::read(path)
    }
}

fn read_file_or_stdin(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    read_file(path)
}

fn write_file_or_stdout(path: &PathBuf, data: &[u8]) -> std::io::Result<()> {
    if path == &PathBuf::from("-") {
        std::io::stdout().write_all(data)
    } else {
        std::fs::write(path, data)
    }
}

fn output_exists_and_not_forced(path: &PathBuf, force: bool) -> bool {
    !force && path != &PathBuf::from("-") && path.exists()
}

fn parse_password_spec(v: &str) -> Result<PasswordSpec, String> {
    if v == "-" {
        return Ok(PasswordSpec::Stdin);
    }
    if let Some(rest) = v.strip_prefix("env:") {
        return Ok(PasswordSpec::Env(rest.to_string()));
    }
    if let Some(rest) = v.strip_prefix("file:") {
        return Ok(PasswordSpec::File(rest.into()));
    }
    Err(format!(
        "--password takes -, env:VAR, or file:PATH (got {v:?}); use --password-insecure-arg for literal"
    ))
}

fn read_passphrase(spec: &PasswordSpec) -> Result<Vec<u8>, String> {
    match spec {
        PasswordSpec::Stdin => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            Ok(s.trim_end_matches(|c| c == '\n' || c == '\r').as_bytes().to_vec())
        }
        PasswordSpec::Env(var) => std::env::var(var).map(String::into_bytes).map_err(|e| e.to_string()),
        PasswordSpec::File(p) => {
            let b = std::fs::read(p).map_err(|e| e.to_string())?;
            // Strip single trailing newline.
            let mut b = b;
            if b.last() == Some(&b'\n') {
                b.pop();
                if b.last() == Some(&b'\r') {
                    b.pop();
                }
            }
            Ok(b)
        }
        PasswordSpec::InsecureArg(s) => Ok(s.as_bytes().to_vec()),
    }
}

fn format_error_exit(e: &Error) -> u8 {
    eprintln!("error: {e}");
    match e {
        Error::FormatUnrecognized | Error::FormatUnsupported { .. } => EXIT_FORMAT,
        Error::PayloadTooLarge { .. } => EXIT_CAPACITY,
        Error::HeaderMissing
        | Error::HeaderBadMagic
        | Error::HeaderBadVersion(_)
        | Error::HeaderReservedBitsSet
        | Error::Malformed { .. } => EXIT_EXTRACT,
        Error::BodyCrcMismatch | Error::BadPassphrase => EXIT_EXTRACT,
        Error::PassphraseRequired | Error::UnexpectedPassphrase => EXIT_PASSPHRASE,
        Error::KdfParams { .. } | Error::RngUnavailable => EXIT_GENERIC,
        Error::DensityOutOfRange(_) | Error::PermutationSeedRequired => EXIT_ARGS,
    }
}

/// Interning trick — most scheme ids are string literals; when one comes from
/// a heap `String` we leak it into `'static` to match adapter API shape.
/// Bounded to the handful of schemes the CLI hands down per invocation.
fn static_str(s: &str) -> &'static str {
    match s {
        "bmp-lsb-linear" => "bmp-lsb-linear",
        "bmp-lsb-permuted" => "bmp-lsb-permuted",
        "wav-lsb-linear" => "wav-lsb-linear",
        "wav-lsb-permuted" => "wav-lsb-permuted",
        "png-lsb-linear" => "png-lsb-linear",
        "png-lsb-permuted" => "png-lsb-permuted",
        other => Box::leak(other.to_string().into_boxed_str()),
    }
}
