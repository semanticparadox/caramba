use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "scripts/"]
pub struct Scripts;

impl Scripts {
    pub fn get_setup_node_script() -> Option<String> {
        Self::get("setup_node.sh")
            .and_then(|file| String::from_utf8(file.data.to_vec()).ok())
    }
}
