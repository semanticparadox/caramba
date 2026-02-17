use dialoguer::{Input, Password, Confirm, Select, theme::ColorfulTheme};
use console::style;

#[derive(Debug)]
pub struct InstallConfig {
    pub domain: String,
    pub sub_domain: Option<String>, // For Hub Mode
    pub admin_path: String,
    pub install_dir: String,
    pub db_pass: String,
}

pub fn interactive_setup(hub_mode: bool) -> InstallConfig {
    println!("{}", style("\nConfiguring Caramba...").bold());

    let domain: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Panel Domain (e.g. panel.example.com)")
        .interact_text()
        .unwrap();

    let sub_domain = if hub_mode {
        Some(Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Subscription Domain (e.g. sub.example.com)")
            .interact_text()
            .unwrap())
    } else {
        None
    };

    let admin_path: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Admin Panel Path")
        .default("/admin".into())
        .interact_text()
        .unwrap();
    
    let install_dir: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Installation Directory")
        .default("/opt/caramba".into())
        .interact_text()
        .unwrap();

    let db_pass: String = Password::with_theme(&ColorfulTheme::default())
        .with_prompt("PostgreSQL Database Password")
        .with_confirmation("Confirm Password", "Passwords mismatch")
        .interact()
        .unwrap();

    InstallConfig {
        domain,
        sub_domain,
        admin_path,
        install_dir,
        db_pass,
    }
}

pub fn generate_caddyfile(config: &InstallConfig) -> String {
    let mut caddyfile = format!(
        "{domain} {{\n    reverse_proxy 127.0.0.1:3000\n}}\n",
        domain = config.domain
    );

    if let Some(sub) = &config.sub_domain {
        caddyfile.push_str(&format!(
            "\n{sub} {{\n    reverse_proxy 127.0.0.1:8080\n}}\n"
        ));
    }
    
    caddyfile
}
