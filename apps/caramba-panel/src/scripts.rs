use rust_embed::RustEmbed;

// Embed the universal installer script into the panel binary so the admin UI
// can serve it from /admino4ka/nodes/install-sh without a file on disk.
// The source file is the canonical `scripts/install.sh` at the repo root
// (the one users run via `curl -fsSL ... | sudo bash`). Resolved at
// compile time against CARGO_MANIFEST_DIR = apps/caramba-panel, so
// `../../scripts` lands on the repo root.
#[derive(RustEmbed)]
#[folder = "../../scripts/"]
pub struct Scripts;

impl Scripts {
    pub fn get_universal_install_script() -> Option<String> {
        Self::get("install.sh").and_then(|file| String::from_utf8(file.data.to_vec()).ok())
    }
}
