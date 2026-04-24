//! XChaCha20-Poly1305 + Argon2id AEAD scheme for rsteg.
//!
//! Implements `rsteg_core::CryptoScheme`. Default scheme; `fourcc = b"XCA1"`.
//! Wire format per `specs/06-crypto.md`:
//!
//! ```text
//! offset  size  field
//! 0       1     kdf_id            (2 = Argon2id; 1 = PBKDF2 fallback — reserved)
//! 1       1     kdf_version       (1 = OWASP 2023 params)
//! 2       16    salt              OS RNG
//! 18      24    nonce             OS RNG
//! 42      N     ciphertext
//! 42+N    16    Poly1305 tag
//! ```
//!
//! The `aad` passed to `seal`/`open` is expected to be the 32-byte outer
//! `PayloadHeader`. We additionally bind the 42-byte inner crypto header
//! (kdf_id/version/salt/nonce) into the tag so any header tamper fails
//! authentication (spec 06 §"AAD scope").

#![deny(unsafe_code)]

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit, Payload},
    Key, XChaCha20Poly1305, XNonce,
};
use rsteg_core::{CryptoScheme, Error};
use zeroize::Zeroizing;

/// Wire identifier for this scheme in `PayloadHeader.crypto_fourcc`.
pub const FOURCC: [u8; 4] = *b"XCA1";

/// XChaCha20-Poly1305 + Argon2id AEAD.
#[derive(Debug, Clone)]
pub struct XChaCha20Argon2id {
    argon_m_cost: u32,
    argon_t_cost: u32,
    argon_p_cost: u32,
}

impl XChaCha20Argon2id {
    /// Spec-default production parameters (Argon2id m=64MiB, t=3, p=1).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            argon_m_cost: 65_536,
            argon_t_cost: 3,
            argon_p_cost: 1,
        }
    }

    /// Low-cost parameters for the test suite. These are NOT safe for
    /// production use — the whole point of Argon2 is the cost factor.
    #[must_use]
    pub const fn new_testing() -> Self {
        Self {
            argon_m_cost: 8, // KiB
            argon_t_cost: 1,
            argon_p_cost: 1,
        }
    }

    /// Seal with caller-supplied salt and nonce. Deterministic; intended for
    /// known-answer tests and benchmarks that need reproducible ciphertext.
    pub fn seal_with_rng(
        &self,
        plaintext: &[u8],
        passphrase: &[u8],
        outer_aad: &[u8],
        salt: [u8; 16],
        nonce: [u8; 24],
    ) -> Result<Vec<u8>, Error> {
        let mut out = Vec::with_capacity(42 + plaintext.len() + 16);
        out.push(2); // kdf_id = Argon2id
        out.push(1); // kdf_version
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce);
        self.seal_inner(plaintext, passphrase, outer_aad, &mut out)?;
        Ok(out)
    }

    fn seal_inner(
        &self,
        plaintext: &[u8],
        passphrase: &[u8],
        outer_aad: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        // `out` already has [kdf_id, kdf_ver, salt(16), nonce(24)] written — 42 bytes.
        debug_assert_eq!(out.len(), 42);
        let inner_hdr = &out[..42];
        let salt = &inner_hdr[2..18];
        let nonce_bytes: [u8; 24] = inner_hdr[18..42].try_into().unwrap();

        let key = derive_key(passphrase, salt, self)?;
        let aad = build_aad(outer_aad, inner_hdr);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&*key));
        let nonce = XNonce::from_slice(&nonce_bytes);

        let mut buf = plaintext.to_vec();
        let tag = cipher
            .encrypt_in_place_detached(nonce, &aad, &mut buf)
            .map_err(|_| Error::RngUnavailable)?; // AEAD seal can only fail on huge inputs
        out.extend_from_slice(&buf);
        out.extend_from_slice(&tag);
        Ok(())
    }
}

impl Default for XChaCha20Argon2id {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoScheme for XChaCha20Argon2id {
    fn id(&self) -> &'static str {
        "xchacha20-argon2id"
    }

    fn fourcc(&self) -> [u8; 4] {
        FOURCC
    }

    fn seal(&self, plaintext: &[u8], passphrase: &[u8], aad: &[u8]) -> Result<Vec<u8>, Error> {
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 24];
        getrandom::getrandom(&mut salt).map_err(|_| Error::RngUnavailable)?;
        getrandom::getrandom(&mut nonce).map_err(|_| Error::RngUnavailable)?;
        self.seal_with_rng(plaintext, passphrase, aad, salt, nonce)
    }

    fn open(&self, ciphertext: &[u8], passphrase: &[u8], outer_aad: &[u8]) -> Result<Vec<u8>, Error> {
        if ciphertext.len() < 42 + 16 {
            // Spec 07 timing test: even the "too short" path must not run
            // measurably faster than the real path. We dummy-derive a key
            // against a fixed salt so an attacker can't distinguish
            // "malformed" from "bad tag" by wall clock.
            let _ = derive_key(passphrase, &[0u8; 16], self);
            return Err(Error::BadPassphrase);
        }

        // Intentionally no explicit check on kdf_id / kdf_version here —
        // those bytes are bound into AAD below, so any tamper fails the
        // Poly1305 tag compare anyway. Skipping the check keeps the
        // open-path timing uniform across all failure modes (spec 06
        // §"Open flow" + spec 07 §"Timing-side-channel test").
        let inner_hdr = &ciphertext[..42];
        let salt = &inner_hdr[2..18];
        let nonce_bytes: [u8; 24] = inner_hdr[18..42].try_into().unwrap();
        let tag_offset = ciphertext.len() - 16;
        let ct_body = &ciphertext[42..tag_offset];
        let tag = &ciphertext[tag_offset..];

        let key = derive_key(passphrase, salt, self)?;
        let aad = build_aad(outer_aad, inner_hdr);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&*key));
        let nonce = XNonce::from_slice(&nonce_bytes);

        let mut buf = ct_body.to_vec();
        cipher
            .decrypt_in_place_detached(
                nonce,
                &aad,
                &mut buf,
                chacha20poly1305::Tag::from_slice(tag),
            )
            .map_err(|_| Error::BadPassphrase)?;
        Ok(buf)
    }
}

/// Derive a 32-byte key from passphrase + salt via Argon2id with the scheme's
/// configured cost parameters. Wraps the key in `Zeroizing` so it scrubs on drop.
fn derive_key(
    passphrase: &[u8],
    salt: &[u8],
    cfg: &XChaCha20Argon2id,
) -> Result<Zeroizing<[u8; 32]>, Error> {
    let params = Params::new(cfg.argon_m_cost, cfg.argon_t_cost, cfg.argon_p_cost, Some(32))
        .map_err(|_| Error::KdfParams { detail: "argon2 params out of range" })?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(passphrase, salt, &mut *key)
        .map_err(|_| Error::KdfParams { detail: "argon2 derive" })?;
    Ok(key)
}

/// AAD = outer 32-byte header || 42-byte inner crypto header (kdf/ver/salt/nonce).
/// Binds every tamperable header byte into the Poly1305 tag.
fn build_aad(outer: &[u8], inner_hdr: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(outer.len() + inner_hdr.len());
    v.extend_from_slice(outer);
    v.extend_from_slice(inner_hdr);
    v
}

// `Payload` is unused for in-place detached AEAD but the import guards
// against accidental removal if we migrate to the combined form.
const _: fn(Payload<'_, '_>) = |_| {};
