//! ed25519 signing-key loading and the `keygen` helper.
//!
//! Operator responsibility: real key material is generated ONCE by the operator
//! (via `caramba-license keygen`, run intentionally) and stored outside the
//! repo. The server loads the private key at startup; the matching public key
//! is what operators bake into the installer as `CARAMBA_LICENSE_PUBKEY`.

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::spki::EncodePublicKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use pkcs8::LineEnding;

/// Load an ed25519 signing key from a PKCS#8 PEM file on disk.
pub fn load_signing_key_pem(path: &str) -> Result<SigningKey> {
    let pem =
        std::fs::read_to_string(path).with_context(|| format!("reading signing key {path}"))?;
    let key = SigningKey::from_pkcs8_pem(&pem)
        .map_err(|e| anyhow::anyhow!("parsing PKCS#8 PEM signing key {path}: {e}"))?;
    Ok(key)
}

/// Generate a fresh ed25519 keypair and write the private key (PKCS#8 PEM) to
/// `out_path`. Returns the matching public key as base64 (the value operators
/// set as `CARAMBA_LICENSE_PUBKEY`). Refuses to overwrite an existing file.
pub fn keygen(out_path: &str) -> Result<String> {
    if std::path::Path::new(out_path).exists() {
        bail!("refusing to overwrite existing key file {out_path}");
    }
    let signing_key = SigningKey::generate(&mut rand_core::OsRng);

    let pem = signing_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("encoding PKCS#8 PEM: {e}"))?;
    std::fs::write(out_path, pem.as_bytes())
        .with_context(|| format!("writing signing key {out_path}"))?;

    // Restrict permissions to owner read/write on unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(out_path, perms);
    }

    Ok(pubkey_b64(&signing_key))
}

/// Render the public (verifying) key of a signing key as standard base64.
pub fn pubkey_b64(signing_key: &SigningKey) -> String {
    let raw = signing_key.verifying_key().to_bytes();
    base64::engine::general_purpose::STANDARD.encode(raw)
}

/// Render the public key as SPKI PEM (for operators who prefer PEM distribution).
pub fn pubkey_pem(signing_key: &SigningKey) -> Result<String> {
    let pem = signing_key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("encoding public key PEM: {e}"))?;
    Ok(pem)
}
