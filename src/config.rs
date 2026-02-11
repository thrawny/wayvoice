use log::debug;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    #[default]
    Groq,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub provider: Provider,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default)]
    pub groq_api_key: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_true")]
    pub use_default_replacements: bool,
    #[serde(default)]
    pub replacements: HashMap<String, String>,
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("wayvoice.toml")
}

fn default_true() -> bool {
    true
}

fn default_prompt() -> String {
    "I'm working on the NixOS configuration with Home Manager. \
     Let me check the Neovim setup in LazyVim. \
     Claude Code suggested refactoring the TypeScript and Rust code. \
     The Hyprland keybindings need updating, same with the Niri config. \
     I'll use tmux and Ghostty for the terminal session. \
     The Kubernetes deployment needs the PostgreSQL migration to run first. \
     Let me check the GitHub pull request and run the CI workflow."
        .to_string()
}

fn default_replacements() -> HashMap<String, String> {
    [
        // Wayland compositors
        ("hyperland", "Hyprland"),
        ("hyper land", "Hyprland"),
        ("neary", "Niri"),
        // Editors
        ("neovim", "Neovim"),
        ("neo vim", "Neovim"),
        ("lazy vim", "LazyVim"),
        ("lazyvim", "LazyVim"),
        // Nix
        ("nix os", "NixOS"),
        ("home manager", "Home Manager"),
        // Claude
        ("cloude code", "Claude Code"),
        ("cloud code", "Claude Code"),
        ("cloudmd", "CLAUDE.md"),
        ("claudemd", "CLAUDE.md"),
        ("weybar", "waybar"),
        ("vtype", "wtype"),
        ("jus", "just"),
        // Apps
        ("ghosty", "Ghostty"),
        ("sunbrowser", "Zen browser"),
        ("tail net", "tailnet"),
        ("urinal", "journal"),
        ("pmpm", "pnpm"),
        ("LTAB", "Alt Tab"),
        (".file", "dotfile"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn load_config() -> Config {
    let path = config_path();
    let mut config = if let Ok(content) = std::fs::read_to_string(&path) {
        match toml::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to parse {path:?}: {e}");
                Config::default()
            }
        }
    } else {
        Config::default()
    };

    // Allow env var to override provider
    if let Ok(provider) = std::env::var("VOICE_PROVIDER") {
        config.provider = match provider.to_lowercase().as_str() {
            "groq" => Provider::Groq,
            "openai" => Provider::Openai,
            _ => config.provider,
        };
    }

    if config.prompt.is_empty() {
        config.prompt = default_prompt();
    }

    // Merge user replacements on top of defaults unless disabled
    if config.use_default_replacements {
        let mut replacements = default_replacements();
        replacements.extend(std::mem::take(&mut config.replacements));
        config.replacements = replacements;
    }

    debug!("provider={:?}", config.provider);
    config
}
