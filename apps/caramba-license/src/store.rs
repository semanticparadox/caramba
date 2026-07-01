//! File-backed key store for the license control plane.
//!
//! The store is a single JSON file mapping `license_key -> KeyEntry`. It holds
//! the tier, expiry, limits, an optional seat cap, and the bound instance id
//! (set on first successful activation). Real key material and hosting are the
//! operator's job; this store is only the issuance + binding ledger.
//!
//! Concurrency: activation binds an instance id by reading, mutating, and
//! writing the file under a process-wide async mutex held by the caller
//! (see `state::ServerState`). Writes are atomic (temp file + rename).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use caramba_shared::license::{LicenseLimits, LicenseTier};

/// One issued license key and its binding state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEntry {
    pub tier: LicenseTier,
    pub expires_at: DateTime<Utc>,
    pub limits: LicenseLimits,
    /// Number of distinct instances allowed. `1` is single-seat (default).
    /// `0` means unlimited seats (no binding enforced).
    #[serde(default = "default_seats")]
    pub seats: u32,
    /// Instance ids bound to this key so far (first activation binds).
    #[serde(default)]
    pub bound_instance_ids: Vec<String>,
    /// Operator note, e.g. who the key was issued to. Never sent to clients.
    #[serde(default)]
    pub note: Option<String>,
}

fn default_seats() -> u32 {
    1
}

impl KeyEntry {
    /// Whether `instance_id` is already bound to this key.
    pub fn is_bound(&self, instance_id: &str) -> bool {
        self.bound_instance_ids.iter().any(|i| i == instance_id)
    }

    /// Whether a new instance id may still bind (seat capacity remains).
    /// `seats == 0` means unlimited.
    pub fn has_free_seat(&self) -> bool {
        self.seats == 0 || (self.bound_instance_ids.len() as u32) < self.seats
    }
}

/// The whole key store: license_key -> entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyStore {
    #[serde(default)]
    pub keys: HashMap<String, KeyEntry>,
}

impl KeyStore {
    /// Load the store from `path`. A missing file yields an empty store so a
    /// fresh server starts cleanly; the first issued key creates the file.
    pub fn load(path: &Path) -> Result<KeyStore> {
        if !path.exists() {
            return Ok(KeyStore::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading key store {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(KeyStore::default());
        }
        let store: KeyStore = serde_json::from_str(&raw)
            .with_context(|| format!("parsing key store {}", path.display()))?;
        Ok(store)
    }

    /// Persist the store to `path` atomically (write temp, then rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context("serializing key store")?;
        let tmp = tmp_path(path);
        std::fs::write(&tmp, json.as_bytes())
            .with_context(|| format!("writing temp key store {}", tmp.display()))?;

        // Restrict the store to owner read/write before it is committed. The
        // store holds issued keys + instance bindings, so it must not be world-
        // or group-readable. Set on the temp file so the committed inode is
        // already 0600 (no readable window between rename and chmod). Mirrors
        // the keygen permission handling in keys.rs.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, perms)
                .with_context(|| format!("setting permissions on key store {}", tmp.display()))?;
        }

        std::fs::rename(&tmp, path)
            .with_context(|| format!("committing key store {}", path.display()))?;
        Ok(())
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}
