//! Shared server state: the signing key, the key store path, and a mutex that
//! serializes read-modify-write activation so instance binding is race free.

use std::path::PathBuf;
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ServerState {
    /// ed25519 private signing key. Server-side only; never leaves the process.
    pub signing_key: Arc<SigningKey>,
    /// Path to the JSON key store file.
    pub store_path: PathBuf,
    /// Serializes activation so concurrent first-activations of the same key
    /// cannot both bind a different instance id.
    pub store_lock: Arc<Mutex<()>>,
}
