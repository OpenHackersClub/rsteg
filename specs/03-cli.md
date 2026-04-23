## CLI surface

Binary name: `rsteg`. Hand-parsed with `lexopt`. Text output on stdout, diagnostics on stderr. `--json` on every subcommand emits structured output to stdout instead.

### Synopsis

```
rsteg <VERB> [OPTIONS]

verbs:
  embed        Hide a payload inside a carrier file.
  extract      Recover a payload from a stego file.
  inspect      Combined format + scheme detection + capacity report.
  list         List compiled-in formats, schemes, and crypto.
  help         Show help for a verb.
  version      Print build + feature set.
```

**Verb consolidation note:** earlier drafts split inspect into `detect` + `info` + `capacity`. These collapsed into one `inspect` verb with mode flags — they share the same detection pipeline and the same `--json` schema is cleaner for scripting.

### Global flags

| Flag | Meaning |
|------|---------|
| `--json`           | Machine-readable output. Schema versioned by `rsteg --version --json`. |
| `--quiet`, `-q`    | Suppress progress messages on stderr. |
| `--verbose`, `-v`  | Debug output on stderr. Repeatable (`-vv`). |
| `--no-color`       | Strip ANSI. Also respects `NO_COLOR=1` env. |
| `--help`, `-h`     | Help for current verb. |
| `--version`, `-V`  | Version + enabled features. |

### `rsteg embed`

```
rsteg embed --in COVER --payload FILE [--payload FILE ...] --out STEGO [--password SPEC] [options]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--in`, `-i` PATH        |   | Cover file (required). Format autodetected unless `--format` set. |
| `--payload`, `-p` PATH   |   | Payload file. Use `-` for stdin. Repeatable (multi-file — see below). Required unless `--payload-text`. |
| `--payload-text` STR     |   | Inline payload string. Conflicts with `--payload`. |
| `--out`, `-o` PATH       |   | Output stego file. Use `-` for stdout. Required. |
| `--password` SPEC        | (none) | Passphrase source. `-` = stdin, `env:VAR` = env var, `file:PATH` = file contents (trailing newline stripped). Never accepts literal inline unless `--password-insecure-arg` is set. If omitted, no encryption. |
| `--password-insecure-arg` STR |  | Literal passphrase in argv. Ugly on purpose; emits a stderr warning; intended for non-interactive CI systems that inject secrets via argv. Documented as "do not use interactively." |
| `--format` FMT           | auto | Override detection: `png`, `bmp`, `wav`, `jpeg`. |
| `--scheme` NAME          | see below | Embedding scheme. Default: the adapter's default permuted scheme when `--password` is supplied, linear scheme otherwise. |
| `--crypto` NAME          | see below | Explicit crypto scheme. Default: `xchacha20-argon2id` iff `--password` is supplied, `none` otherwise. Not feature-gated — if the `crypto` feature is off and `--password` is given, the CLI errors out cleanly at runtime. |
| `--density` N            | 1        | 1 or 2. `3` and `4` require `--allow-aggressive-density`. |
| `--allow-aggressive-density` | off | Unlocks `--density 3` and `--density 4`. Carries visible-distortion warnings. |
| `--no-header`            | off      | Skip the 32-byte framing header. Raw bits only. See caveats below. |
| `--force`                | off      | Overwrite `--out` if it exists. |
| `--force-unsafe`         | off      | Required to combine `--no-header` with `--crypto none`. |

**Multi-file payloads** (when `--payload` is given more than once): rsteg writes a tiny on-disk manifest ahead of the payload bytes:

```
files_count: u16 BE
for each file:
  name_len: u16 BE
  name: utf-8 bytes
  data_len: u32 BE
  data: raw bytes
```

This manifest sits between `PayloadHeader` and the body bytes (or inside the AEAD ciphertext when encrypted). Consumers parse it on extract; single-payload files have `files_count = 1` and a synthetic name derived from `--payload` (or "payload" for stdin). Scope: phase 1.

Exits 4 (`PayloadTooLarge`) before touching the output file.

### `rsteg extract`

```
rsteg extract --in STEGO --out PAYLOAD [--password SPEC] [options]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--in`, `-i` PATH   |   | Stego file (required). |
| `--out`, `-o` PATH  |   | Payload output. Use `-` for stdout. Required when manifest has a single file; see `--out-dir`. |
| `--out-dir` PATH    |   | Required when a multi-file manifest is extracted. Files written with their manifest names. |
| `--password` SPEC   |   | As above. Required for encrypted payloads. |
| `--format` FMT      | auto | Override detection. |
| `--scheme` NAME     | auto | Override when scheme can't be detected. |
| `--no-header`       | off  | Raw extraction, treat output as opaque bits (size comes from `--length`). |
| `--length` N        |      | Required with `--no-header`. Bytes to read. |
| `--force`           | off  | Overwrite `--out`. |
| `--force-unsafe`    | off  | Required for `--no-header` without a passphrase (large attacker-controlled bit buckets). |

**Plaintext vs password:**
- If `--password` is supplied but the header reports `flags.encrypted = 0` → `Error::UnexpectedPassphrase`. The CLI translates this to exit 6 and a helpful message: "this file was not encrypted; omit --password."
- If `flags.encrypted = 1` but no `--password` → `Error::PassphraseRequired`, exit 6.

### `rsteg inspect`

```
rsteg inspect --in FILE [--mode MODE] [--password SPEC]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--in` PATH    |   | File to inspect (required). |
| `--mode` M     | `full` | `detect` (format + scheme hits only), `capacity` (just capacity), `info` (carrier metadata), `full` (everything). |
| `--password` SPEC |  | Enables `Confirmed` detection confidence for rsteg and steghide files. Omitted = detection is keyless. |

JSON shape (`--mode full`):
```json
{
  "format":    { "id": "bmp", "variant": "bmp24-uncompressed",
                 "width": 512, "height": 512, "channels": 3 },
  "stego":     [
    { "scheme": "bmp-lsb-permuted", "source": "RstegHeader", "level": "Confirmed",
      "needs_password": false },
    { "scheme": "steghide",         "source": "ChiSquare",   "level": "Suspected",
      "needs_password": true }
  ],
  "capacity":  { "scheme": "bmp-lsb-linear", "density": 1,
                 "bytes_raw": 98304, "bytes_usable_plain": 98272,
                 "bytes_usable_aead": 98214 }
}
```

Confidence levels (see spec 04): `Negative` / `Inconclusive` / `Suspected` / `Confirmed`. Sources: `RstegHeader`, `SteghideMagic`, `ChiSquare`, `EntropyDelta`, `FormatEligible`.

### `rsteg list`

```
rsteg list formats
rsteg list schemes
rsteg list crypto
rsteg list all
```

Enumerates what was compiled in. Useful for bug reports and CI.

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Generic error / RNG unavailable |
| 2 | Argument error (missing flag, bad value, passphrase in argv without opt-in) |
| 3 | Carrier format not recognized or not supported in this build |
| 4 | Payload too large for carrier at requested density |
| 5 | Extraction failed — no rsteg header, corrupt header, or scheme mismatch |
| 6 | Passphrase error — wrong passphrase, file was/wasn't encrypted vs caller expectation, or tampered ciphertext |
| 7 | I/O error |
| 8 | Policy error (e.g. refused to overwrite without `--force`, aggressive density without `--allow-aggressive-density`) |

### Passphrase handling rules

- **Default-strict on argv.** A literal passphrase in argv is rejected at parse time unless `--password-insecure-arg` is used. CI with secret injection via argv is a real workflow; we provide the explicit opt-in with a deliberately ugly name and a stderr warning.
- **stdin (`--password -`)**: read until EOF, strip one trailing `\n` or `\r\n`, zeroize buffer after use. When stdin is a TTY, terminal echo is suppressed (`termios` on Unix, `SetConsoleMode` on Windows). The echo-suppression code path is the only place `rsteg-cli` touches OS APIs other than file I/O; it's ~30 LOC, hand-rolled, behind `#[cfg(unix)]` / `#[cfg(windows)]`.
- **`env:VAR`**: read once, buffer zeroized, stderr warning reminds user to `unset`. Documented as "use only in single-tenant automation."
- **`file:PATH`**: open with `O_NOFOLLOW` (Unix). Stat the file and reject if mode has any bits in 0077 unless `--force-unsafe`. Read whole file, strip one trailing newline, zeroize.
- **Zeroization**: via `zeroize::Zeroizing<Vec<u8>>`. Spec 02 allowlists the crate; hand-rolled zeroing is unsound in safe Rust (see spec 06 discussion).

### Help text rules

- `rsteg --help` shows verbs + global flags, nothing else.
- `rsteg embed --help` shows the full verb spec.
- Errors on bad args include a "did you mean" suggestion computed via Levenshtein distance ≤ 2 (~20 lines of hand-written code).
- No colored output unless stdout is a TTY and `NO_COLOR` is unset. Hand-rolled ANSI helper, no `owo-colors`/`colored` dep.

### Stability

- Exit codes and `--json` schema follow semver from 1.0. Additions (new fields, new verbs) are minor; breaking changes are major.
- Human-readable text output is NOT stable; scripts must use `--json`.
- Pre-1.0: `0.1.x → 0.1.(x+1)` is compatible; `0.1 → 0.2` is the breaking boundary.
