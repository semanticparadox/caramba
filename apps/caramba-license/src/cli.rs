//! CLI surface for the license control plane.
//!
//! Subcommands:
//!   - `serve`     run the activation HTTP server
//!   - `issue`     issue a license key for a tier + duration into the key store
//!   - `keygen`    generate the ed25519 signing key (operator runs this once)
//!   - `pubkey`    print the public key for a given signing key (for the installer)
//!   - `list`      list issued keys in the store

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "caramba-license",
    about = "Caramba license control plane: activation server and key issuance.",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the activation HTTP server (serves POST /v1/activate).
    Serve(ServeArgs),
    /// Issue a license key for a tier and duration into the key store.
    Issue(IssueArgs),
    /// Generate a new ed25519 signing key (run once, store the key safely).
    Keygen(KeygenArgs),
    /// Print the public key (CARAMBA_LICENSE_PUBKEY) for a signing key.
    Pubkey(PubkeyArgs),
    /// List issued keys in the store.
    List(ListArgs),
}

#[derive(clap::Args, Debug)]
pub struct ServeArgs {
    /// Path to the ed25519 signing key in PKCS#8 PEM.
    #[arg(long, env = "CARAMBA_LICENSE_SIGNING_KEY")]
    pub signing_key: String,
    /// Path to the JSON key store file.
    #[arg(long, env = "CARAMBA_LICENSE_STORE", default_value = "keystore.json")]
    pub store: String,
    /// Bind address.
    #[arg(long, env = "CARAMBA_LICENSE_BIND", default_value = "0.0.0.0:8088")]
    pub bind: String,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum TierArg {
    Free,
    Pro,
}

#[derive(clap::Args, Debug)]
pub struct IssueArgs {
    /// Path to the JSON key store file (created if missing).
    #[arg(long, env = "CARAMBA_LICENSE_STORE", default_value = "keystore.json")]
    pub store: String,
    /// Tier to grant.
    #[arg(long, value_enum, default_value = "pro")]
    pub tier: TierArg,
    /// Validity in days from now.
    #[arg(long, default_value = "365")]
    pub days: i64,
    /// Number of instances allowed (seats). 1 is single-seat, 0 is unlimited.
    #[arg(long, default_value = "1")]
    pub seats: u32,
    /// Explicit license key string. If omitted, a random key is generated.
    #[arg(long)]
    pub key: Option<String>,
    /// Operator note (who the key is for). Never sent to clients.
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct KeygenArgs {
    /// Output path for the private signing key (PKCS#8 PEM). Not overwritten.
    #[arg(long, default_value = "license_signing_key.pem")]
    pub out: String,
}

#[derive(clap::Args, Debug)]
pub struct PubkeyArgs {
    /// Path to the ed25519 signing key in PKCS#8 PEM.
    #[arg(long, env = "CARAMBA_LICENSE_SIGNING_KEY")]
    pub signing_key: String,
    /// Emit SPKI PEM instead of base64.
    #[arg(long)]
    pub pem: bool,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Path to the JSON key store file.
    #[arg(long, env = "CARAMBA_LICENSE_STORE", default_value = "keystore.json")]
    pub store: String,
}
