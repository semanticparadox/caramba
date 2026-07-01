//! Caramba license control plane.
//!
//! A small ed25519 activation server plus key-issuance CLI. It serves
//! `POST /v1/activate`, looks up issued license keys, enforces expiry, binds an
//! instance id to a key on first activation, and returns a signed
//! [`caramba_shared::license::ActivationResponse`] that the panel verifies with
//! its baked public key.
//!
//! Real key material and hosting are the operator's job. Generate the signing
//! key once with `caramba-license keygen` and keep it off the repo.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::routing::{get, post};
use chrono::{Duration, Utc};
use clap::Parser;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use caramba_shared::license::{LicenseLimits, LicenseTier};

mod activate;
mod cli;
mod keys;
mod state;
mod store;

use cli::{Cli, Command, IssueArgs, KeygenArgs, ListArgs, PubkeyArgs, ServeArgs, TierArg};
use state::ServerState;
use store::{KeyEntry, KeyStore};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => serve(args).await,
        Command::Issue(args) => issue(args),
        Command::Keygen(args) => keygen(args),
        Command::Pubkey(args) => pubkey(args),
        Command::List(args) => list(args),
    }
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "caramba_license=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn serve(args: ServeArgs) -> Result<()> {
    init_tracing();

    let signing_key = keys::load_signing_key_pem(&args.signing_key)
        .context("loading signing key for serve")?;
    tracing::info!("loaded signing key, public key: {}", keys::pubkey_b64(&signing_key));

    // Validate the store loads (creates an empty in-memory store if missing).
    let store_path = PathBuf::from(&args.store);
    let _ = KeyStore::load(&store_path).context("loading key store for serve")?;

    let state = ServerState {
        signing_key: Arc::new(signing_key),
        store_path,
        store_lock: Arc::new(Mutex::new(())),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/activate", post(activate::activate))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("binding {}", args.bind))?;
    tracing::info!("license server listening on {}", args.bind);
    axum::serve(listener, app).await.context("axum serve")?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

fn tier_from_arg(t: TierArg) -> LicenseTier {
    match t {
        TierArg::Free => LicenseTier::Free,
        TierArg::Pro => LicenseTier::Pro,
    }
}

fn issue(args: IssueArgs) -> Result<()> {
    let store_path = PathBuf::from(&args.store);
    let mut store = KeyStore::load(&store_path).context("loading key store for issue")?;

    let tier = tier_from_arg(args.tier);
    let license_key = match args.key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => generate_key(),
    };

    if store.keys.contains_key(&license_key) {
        anyhow::bail!("license key already exists in the store");
    }

    let expires_at = Utc::now() + Duration::days(args.days);
    let entry = KeyEntry {
        tier,
        expires_at,
        limits: LicenseLimits::for_tier(tier),
        seats: args.seats,
        bound_instance_ids: Vec::new(),
        note: args.note,
    };
    store.keys.insert(license_key.clone(), entry);
    store.save(&store_path).context("saving key store after issue")?;

    println!("Issued license key:");
    println!("  key:        {license_key}");
    println!("  tier:       {}", tier.as_str());
    println!("  expires_at: {}", expires_at.to_rfc3339());
    println!("  seats:      {}", args.seats);
    println!("  store:      {}", store_path.display());
    Ok(())
}

/// Generate a random, human-handleable license key:
/// `CRMB-XXXX-XXXX-XXXX-XXXX` from a 16-byte random seed (crockford-ish).
fn generate_key() -> String {
    use rand_core::RngCore;
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut raw = [0u8; 16];
    rand_core::OsRng.fill_bytes(&mut raw);
    let mut chars: Vec<char> = raw
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    // 16 chars -> CRMB-XXXX-XXXX-XXXX-XXXX
    let g: String = chars.drain(..).collect();
    format!(
        "CRMB-{}-{}-{}-{}",
        &g[0..4],
        &g[4..8],
        &g[8..12],
        &g[12..16]
    )
}

fn keygen(args: KeygenArgs) -> Result<()> {
    let pub_b64 = keys::keygen(&args.out).context("generating signing key")?;
    println!("Generated ed25519 signing key.");
    println!("  private key (PKCS#8 PEM): {}", args.out);
    println!("  CARAMBA_LICENSE_PUBKEY:   {pub_b64}");
    println!();
    println!("Keep the private key off the repo and readable only by the server user.");
    println!("Bake the public key above into the installer as CARAMBA_LICENSE_PUBKEY.");
    Ok(())
}

fn pubkey(args: PubkeyArgs) -> Result<()> {
    let signing_key =
        keys::load_signing_key_pem(&args.signing_key).context("loading signing key for pubkey")?;
    if args.pem {
        print!("{}", keys::pubkey_pem(&signing_key)?);
    } else {
        println!("{}", keys::pubkey_b64(&signing_key));
    }
    Ok(())
}

fn list(args: ListArgs) -> Result<()> {
    let store_path = PathBuf::from(&args.store);
    let store = KeyStore::load(&store_path).context("loading key store for list")?;
    if store.keys.is_empty() {
        println!("No keys issued in {}.", store_path.display());
        return Ok(());
    }
    for (key, entry) in &store.keys {
        let bound = if entry.bound_instance_ids.is_empty() {
            "unbound".to_string()
        } else {
            entry.bound_instance_ids.join(", ")
        };
        println!(
            "{key}  tier={} expires={} seats={} bound=[{bound}]{}",
            entry.tier.as_str(),
            entry.expires_at.to_rfc3339(),
            entry.seats,
            entry
                .note
                .as_ref()
                .map(|n| format!(" note={n}"))
                .unwrap_or_default()
        );
    }
    Ok(())
}
