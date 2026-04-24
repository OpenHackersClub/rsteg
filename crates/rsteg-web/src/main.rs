//! rsteg demo site — dev-only showcase + interactive embed/extract.
//!
//! Routes:
//!   GET  /                  — landing page (HTML)
//!   GET  /api/presets       — JSON list of available preset carriers
//!   GET  /api/preset/:id    — serve a preset BMP
//!   GET  /api/cover         — legacy alias for /api/preset/gradient
//!   POST /api/embed         — multipart: cover file OR preset= + secret -> stego BMP
//!   POST /api/extract       — multipart: stego file -> extracted body as JSON
//!   POST /api/inspect       — multipart: file -> JSON diagnostics

use axum::{
    body::Bytes,
    extract::{Multipart, Path},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{
    shuffle, Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, Prng, SchemeFourcc,
};
use serde::{Deserialize, Serialize};

const INDEX_HTML: &str = include_str!("index.html");
const ALGO_INDEX_HTML: &str = include_str!("pages/algo_index.html");
const ALGO_LSB_LINEAR_HTML: &str = include_str!("pages/lsb_linear.html");
const ALGO_LSB_PERMUTED_HTML: &str = include_str!("pages/lsb_permuted.html");
const ALGO_PAYLOAD_HEADER_HTML: &str = include_str!("pages/payload_header.html");
const ALGO_AEAD_HTML: &str = include_str!("pages/aead.html");

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/presets", get(list_presets))
        .route("/api/preset/:id", get(get_preset))
        .route("/api/cover", get(cover_alias))
        .route("/api/embed", post(embed_handler))
        .route("/api/extract", post(extract_handler))
        .route("/api/inspect", post(inspect_handler))
        .route("/algo", get(algo_index))
        .route("/algo/lsb-linear", get(algo_lsb_linear))
        .route("/algo/lsb-permuted", get(algo_lsb_permuted))
        .route("/algo/payload-header", get(algo_payload_header))
        .route("/algo/aead", get(algo_aead))
        .route("/api/algo/permutation", post(permutation_trace))
        .route("/api/algo/header-encode", post(header_encode));

    let addr = std::env::var("RSTEG_WEB_ADDR").unwrap_or_else(|_| "127.0.0.1:3456".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    eprintln!("rsteg-web listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn algo_index() -> Html<&'static str> {
    Html(ALGO_INDEX_HTML)
}

async fn algo_lsb_linear() -> Html<&'static str> {
    Html(ALGO_LSB_LINEAR_HTML)
}

async fn algo_lsb_permuted() -> Html<&'static str> {
    Html(ALGO_LSB_PERMUTED_HTML)
}

async fn algo_payload_header() -> Html<&'static str> {
    Html(ALGO_PAYLOAD_HEADER_HTML)
}

async fn algo_aead() -> Html<&'static str> {
    Html(ALGO_AEAD_HTML)
}

// ---------------- Algo visualization APIs ----------------

#[derive(Deserialize)]
struct PermutationReq {
    /// 16-char hex string, lower or upper case.
    seed_hex: String,
    /// Size of the embedding-unit universe (e.g. 64 for a 64-byte cover).
    len: usize,
}

#[derive(Serialize)]
struct PermutationResp {
    seed: String,
    len: usize,
    /// `order[i]` is the carrier index visited at step `i`.
    order: Vec<usize>,
}

async fn permutation_trace(Json(req): Json<PermutationReq>) -> Result<Json<PermutationResp>, (StatusCode, String)> {
    if req.len == 0 || req.len > 4096 {
        return Err((StatusCode::BAD_REQUEST, "len must be in 1..=4096".into()));
    }
    let seed = parse_hex_u64(req.seed_hex.trim())
        .ok_or((StatusCode::BAD_REQUEST, "seed_hex must be 1..=16 hex chars".into()))?;
    let mut idx: Vec<usize> = (0..req.len).collect();
    let mut prng = Prng::new(seed);
    shuffle(&mut prng, &mut idx);
    Ok(Json(PermutationResp {
        seed: format!("{seed:016x}"),
        len: req.len,
        order: idx,
    }))
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

#[derive(Deserialize)]
struct HeaderEncodeReq {
    scheme: String,
    density: u8,
    body_hex: String,
    encrypted: bool,
}

#[derive(Serialize)]
struct HeaderEncodeResp {
    hex: String,
    fields: Vec<HeaderField>,
    body_len: u32,
    body_crc32: u32,
}

#[derive(Serialize)]
struct HeaderField {
    name: &'static str,
    offset: usize,
    size: usize,
    value: String,
    note: &'static str,
}

async fn header_encode(Json(req): Json<HeaderEncodeReq>) -> Result<Json<HeaderEncodeResp>, (StatusCode, String)> {
    let scheme = match req.scheme.as_str() {
        "bmp-lsb-linear" => SchemeFourcc::BMP_LSB_LINEAR,
        "bmp-lsb-permuted" => SchemeFourcc::BMP_LSB_PERMUTED,
        "wav-lsb-linear" => SchemeFourcc::WAV_LSB_LINEAR,
        "wav-lsb-permuted" => SchemeFourcc::WAV_LSB_PERMUTED,
        "png-lsb-linear" => SchemeFourcc::PNG_LSB_LINEAR,
        "png-lsb-permuted" => SchemeFourcc::PNG_LSB_PERMUTED,
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown scheme '{other}'"))),
    };
    let density = match req.density {
        1 => Density::Low,
        2 => Density::Moderate,
        3 => Density::Aggressive3,
        4 => Density::Aggressive4,
        _ => return Err((StatusCode::BAD_REQUEST, "density must be 1..=4".into())),
    };
    let body = decode_hex(&req.body_hex)
        .ok_or((StatusCode::BAD_REQUEST, "body_hex must be even-length hex".into()))?;
    if body.len() > 1 << 20 {
        return Err((StatusCode::BAD_REQUEST, "body too large (>1 MiB)".into()));
    }

    let mut hdr = PayloadHeader::plain(scheme, density, &body);
    if req.encrypted {
        hdr.flags |= PayloadHeader::FLAG_ENCRYPTED;
        hdr.body_crc32 = 0;
        hdr.crypto_fourcc = *b"XCA2";
    }
    let bytes = hdr.encode();

    let fields = vec![
        HeaderField { name: "magic",         offset: 0,  size: 4, value: "\"RSTG\"".into(),                                    note: "ASCII tag, never changes" },
        HeaderField { name: "version",       offset: 4,  size: 1, value: format!("0x{:02x}", hdr.version),                     note: "wire version, current = 1" },
        HeaderField { name: "flags",         offset: 5,  size: 1, value: format!("0x{:02x}", hdr.flags),                       note: "bit0 encrypted, bit1 compressed, bit2 permuted" },
        HeaderField { name: "crypto_fourcc", offset: 6,  size: 4, value: ascii_or_hex(&hdr.crypto_fourcc),                      note: "AEAD scheme id; zero for plaintext" },
        HeaderField { name: "scheme_fourcc", offset: 10, size: 4, value: ascii_or_hex(&hdr.scheme_fourcc.0),                    note: "carrier + walk order (BLSL, WLSP, …)" },
        HeaderField { name: "density",       offset: 14, size: 1, value: format!("0x{:02x}", hdr.density),                      note: "bits used per sample, 1..=4" },
        HeaderField { name: "reserved",      offset: 15, size: 1, value: "0x00".into(),                                         note: "must be zero on write, checked on read" },
        HeaderField { name: "body_len",      offset: 16, size: 4, value: format!("{} (0x{:08x})", hdr.body_len, hdr.body_len),  note: "u32 big-endian length of body bytes" },
        HeaderField { name: "body_crc32",    offset: 20, size: 4, value: format!("0x{:08x}", hdr.body_crc32),                   note: "CRC32-IEEE of plaintext body; zero when encrypted" },
        HeaderField { name: "reserved",      offset: 24, size: 8, value: "0x00 × 8".into(),                                     note: "reserved-zero, preserves 32-byte frame" },
    ];

    Ok(Json(HeaderEncodeResp {
        hex: encode_hex_upper(&bytes),
        fields,
        body_len: hdr.body_len,
        body_crc32: hdr.body_crc32,
    }))
}

fn encode_hex_upper(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() {
        return Some(Vec::new());
    }
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn ascii_or_hex(fcc: &[u8; 4]) -> String {
    if fcc.iter().all(|b| b.is_ascii_graphic()) {
        format!("\"{}\"", std::str::from_utf8(fcc).unwrap_or("????"))
    } else {
        format!("0x{:02x}{:02x}{:02x}{:02x}", fcc[0], fcc[1], fcc[2], fcc[3])
    }
}

// ---------------- Presets ----------------

#[derive(Clone, Copy)]
enum Preset {
    Gradient,
    SolidGray,
    Checkerboard,
    Stripes,
    Photo,
    Noise,
}

impl Preset {
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "gradient" => Self::Gradient,
            "solid-gray" => Self::SolidGray,
            "checkerboard" => Self::Checkerboard,
            "stripes" => Self::Stripes,
            "photo" => Self::Photo,
            "noise" => Self::Noise,
            _ => return None,
        })
    }

    fn id(self) -> &'static str {
        match self {
            Self::Gradient => "gradient",
            Self::SolidGray => "solid-gray",
            Self::Checkerboard => "checkerboard",
            Self::Stripes => "stripes",
            Self::Photo => "photo",
            Self::Noise => "noise",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Gradient => "Gradient",
            Self::SolidGray => "Solid gray",
            Self::Checkerboard => "Checkerboard",
            Self::Stripes => "Stripes",
            Self::Photo => "Photo-like",
            Self::Noise => "Random noise",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Gradient => "Soft diagonal gradient. LSB tweaks vanish into the smooth transitions.",
            Self::SolidGray => "Uniform #808080. Any LSB change stands out on chi-square — useful teaching example.",
            Self::Checkerboard => "16×16 tiles. High-contrast edges; LSB flips never cross a tile boundary visibly.",
            Self::Stripes => "Alternating color columns. Shows the density-vs-visibility tradeoff dramatically.",
            Self::Photo => "Multi-frequency sine waves. Approximates photographic noise best of the synthetic set.",
            Self::Noise => "Deterministic PRNG noise. Maximum LSB entropy — hardest cover to statistically detect in.",
        }
    }

    fn render(self, w: u32, h: u32) -> Vec<u8> {
        match self {
            Self::Gradient => make_bmp24(w, h, |x, y| {
                let v = (x as u32 + y as u32 * 2) as u8;
                (v, v.wrapping_add(40), v.wrapping_add(80))
            }),
            Self::SolidGray => make_bmp24(w, h, |_x, _y| (0x80, 0x80, 0x80)),
            Self::Checkerboard => make_bmp24(w, h, |x, y| {
                let on = ((x / 16) + (y / 16)) % 2 == 0;
                if on { (240, 240, 240) } else { (25, 25, 25) }
            }),
            Self::Stripes => make_bmp24(w, h, |x, _y| {
                match x / 16 % 4 {
                    0 => (210, 90, 90),
                    1 => (90, 160, 210),
                    2 => (210, 200, 90),
                    _ => (110, 200, 110),
                }
            }),
            Self::Photo => make_bmp24(w, h, |x, y| {
                // Cheap "natural" texture — mix a few sines across R/G/B.
                let fx = x as f32 / w as f32;
                let fy = y as f32 / h as f32;
                let r = ((fx * 7.0).sin() + (fy * 5.0).cos()) * 0.25 + 0.5;
                let g = ((fx * 11.0 + fy * 3.0).sin()) * 0.25 + 0.5;
                let b = ((fy * 9.0 - fx * 4.0).cos()) * 0.25 + 0.55;
                (
                    (r.clamp(0.0, 1.0) * 230.0) as u8 + 10,
                    (g.clamp(0.0, 1.0) * 230.0) as u8 + 10,
                    (b.clamp(0.0, 1.0) * 210.0) as u8 + 20,
                )
            }),
            Self::Noise => {
                let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
                make_bmp24(w, h, |_x, _y| {
                    state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                    let z = state ^ (state >> 30);
                    let z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                    let z = z ^ (z >> 27);
                    let z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
                    let z = z ^ (z >> 31);
                    let b = z.to_le_bytes();
                    (b[0], b[1], b[2])
                })
            }
        }
    }
}

const ALL_PRESETS: &[Preset] = &[
    Preset::Gradient,
    Preset::Photo,
    Preset::Noise,
    Preset::Stripes,
    Preset::Checkerboard,
    Preset::SolidGray,
];

#[derive(Serialize)]
struct PresetMeta {
    id: &'static str,
    label: &'static str,
    note: &'static str,
    width: u32,
    height: u32,
    url: String,
}

async fn list_presets() -> Json<Vec<PresetMeta>> {
    Json(
        ALL_PRESETS
            .iter()
            .map(|p| PresetMeta {
                id: p.id(),
                label: p.label(),
                note: p.note(),
                width: 256,
                height: 256,
                url: format!("/api/preset/{}", p.id()),
            })
            .collect(),
    )
}

async fn get_preset(Path(id): Path<String>) -> Response {
    let Some(p) = Preset::from_id(&id) else {
        return error_response(StatusCode::NOT_FOUND, format!("unknown preset: {id}"));
    };
    let bmp = p.render(256, 256);
    (
        [
            (header::CONTENT_TYPE, "image/bmp"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        bmp,
    )
        .into_response()
}

async fn cover_alias() -> Response {
    get_preset(Path("gradient".into())).await
}

// ---------------- Embed / Extract / Inspect ----------------

async fn embed_handler(mut mp: Multipart) -> Response {
    let mut cover: Option<Bytes> = None;
    let mut preset_id: Option<String> = None;
    let mut secret: Option<String> = None;
    let mut density_bits: u8 = 1;

    while let Ok(Some(field)) = mp.next_field().await {
        match field.name().unwrap_or_default() {
            "cover" => {
                let bytes = field.bytes().await.unwrap_or_default();
                if !bytes.is_empty() {
                    cover = Some(bytes);
                }
            }
            "preset" => {
                let s = field.text().await.unwrap_or_default();
                if !s.is_empty() {
                    preset_id = Some(s);
                }
            }
            "secret" => {
                secret = Some(field.text().await.unwrap_or_default());
            }
            "density" => {
                if let Ok(d) = field.text().await {
                    density_bits = d.trim().parse().unwrap_or(1).clamp(1, 4);
                }
            }
            _ => {}
        }
    }

    let cover = if let Some(b) = cover {
        b.to_vec()
    } else if let Some(id) = preset_id {
        match Preset::from_id(&id) {
            Some(p) => p.render(256, 256),
            None => return error_response(StatusCode::BAD_REQUEST, format!("unknown preset: {id}")),
        }
    } else {
        Preset::Gradient.render(256, 256)
    };

    let secret = secret.unwrap_or_default();
    let density = match density_bits {
        1 => Density::Low,
        2 => Density::Moderate,
        3 => Density::Aggressive3,
        _ => Density::Aggressive4,
    };

    let header_struct = PayloadHeader::plain(
        SchemeFourcc::BMP_LSB_LINEAR,
        density,
        secret.as_bytes(),
    );
    let framed = header_struct.encode_with(secret.as_bytes());
    let opts = EmbedOpts { scheme: Some("bmp-lsb-linear"), density, seed: None };

    match BMP_ADAPTER.embed(&cover, &framed, &opts) {
        Ok(stego) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/bmp"),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"stego.bmp\""),
                (header::HeaderName::from_static("x-rsteg-framed-bytes"),
                    Box::leak(framed.len().to_string().into_boxed_str()) as &str),
                (header::HeaderName::from_static("x-rsteg-density"),
                    Box::leak(density_bits.to_string().into_boxed_str()) as &str),
            ],
            stego,
        )
            .into_response(),
        Err(e) => error_response(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    }
}

#[derive(Serialize)]
struct ExtractResult {
    secret_utf8: Option<String>,
    secret_hex: String,
    body_len: usize,
    framed_len: usize,
    density: u8,
    scheme_fourcc: String,
}

async fn extract_handler(mut mp: Multipart) -> Response {
    let mut stego: Option<Bytes> = None;
    let mut density_bits: u8 = 1;

    while let Ok(Some(field)) = mp.next_field().await {
        match field.name().unwrap_or_default() {
            "stego" => {
                let bytes = field.bytes().await.unwrap_or_default();
                if !bytes.is_empty() { stego = Some(bytes); }
            }
            "density" => {
                if let Ok(d) = field.text().await {
                    density_bits = d.trim().parse().unwrap_or(1).clamp(1, 4);
                }
            }
            _ => {}
        }
    }

    let Some(stego) = stego else {
        return error_response(StatusCode::BAD_REQUEST, "missing stego file".to_string());
    };
    let density = match density_bits {
        1 => Density::Low,
        2 => Density::Moderate,
        3 => Density::Aggressive3,
        _ => Density::Aggressive4,
    };

    let opts = ExtractOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Some(density),
        skip_header: false,
        raw_bit_count: None,
        seed: None,
    };
    match BMP_ADAPTER.extract(&stego, &opts) {
        Ok(framed) => {
            let header_bytes = &framed[..PayloadHeader::SIZE];
            let header_struct = match PayloadHeader::decode(header_bytes) {
                Ok(h) => h,
                Err(e) => return error_response(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
            };
            let body = &framed[PayloadHeader::SIZE..];
            let utf8 = std::str::from_utf8(body).ok().map(str::to_string);
            let result = ExtractResult {
                secret_utf8: utf8,
                secret_hex: hex_lower(body),
                body_len: body.len(),
                framed_len: framed.len(),
                density: header_struct.density,
                scheme_fourcc: String::from_utf8_lossy(&header_struct.scheme_fourcc.0).into_owned(),
            };
            Json(result).into_response()
        }
        Err(e) => error_response(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    }
}

#[derive(Serialize)]
struct InspectResult {
    recognized_as: Option<String>,
    size_bytes: usize,
    bmp: Option<BmpInfo>,
    capacity_at_density_1: Option<u64>,
}

#[derive(Serialize)]
struct BmpInfo {
    width: u32,
    height: u32,
    bits_per_pixel: u16,
    pixel_offset: u32,
}

async fn inspect_handler(mut mp: Multipart) -> Response {
    let mut file: Option<Bytes> = None;
    while let Ok(Some(field)) = mp.next_field().await {
        if field.name() == Some("file") {
            file = Some(field.bytes().await.unwrap_or_default());
        }
    }
    let Some(file) = file else {
        return error_response(StatusCode::BAD_REQUEST, "missing file".to_string());
    };

    let mut result = InspectResult {
        recognized_as: None,
        size_bytes: file.len(),
        bmp: None,
        capacity_at_density_1: None,
    };

    if BMP_ADAPTER.recognize(&file) && file.len() >= 54 {
        result.recognized_as = Some("bmp".into());
        let width = u32::from_le_bytes(file[18..22].try_into().unwrap());
        let height_signed = i32::from_le_bytes(file[22..26].try_into().unwrap());
        let height = height_signed.unsigned_abs();
        let bpp = u16::from_le_bytes(file[28..30].try_into().unwrap());
        let pixel_offset = u32::from_le_bytes(file[10..14].try_into().unwrap());
        let capacity = (u64::from(width) * u64::from(height) * 3) / 8;
        result.bmp = Some(BmpInfo { width, height, bits_per_pixel: bpp, pixel_offset });
        result.capacity_at_density_1 = Some(capacity);
    }

    Json(result).into_response()
}

// ---------------- Helpers ----------------

#[derive(Serialize)]
struct ErrResp { error: String }

fn error_response(status: StatusCode, msg: String) -> Response {
    (status, Json(ErrResp { error: msg })).into_response()
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xF) as usize] as char);
    }
    s
}

/// Build a 24-bit BMP using a pixel-producer function returning (R,G,B).
fn make_bmp24(width: u32, height: u32, mut px: impl FnMut(u32, u32) -> (u8, u8, u8)) -> Vec<u8> {
    let row_bytes = (width as usize) * 3;
    let stride = (row_bytes + 3) & !3;
    let pixel_bytes = stride * (height as usize);
    let file_size = 54 + pixel_bytes;
    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = px(x, y);
            // BMP bottom-up top-down is indicated by sign(height); our
            // height is positive = top-down.  BMP pixel order is BGR.
            out.push(b);
            out.push(g);
            out.push(r);
        }
        for _ in (width * 3) as usize..stride {
            out.push(0);
        }
    }
    out
}
