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

#[derive(Debug, Deserialize, Default, Clone)]
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
    #[serde(default)]
    #[cfg_attr(not(feature = "hud-ui"), allow(dead_code))]
    pub hud_color: Option<String>,
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
    // Keep this list conservative to avoid accidental rewrites.
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
        // Claude + tooling
        ("cloude code", "Claude Code"),
        ("cloud code", "Claude Code"),
        ("cloudmd", "CLAUDE.md"),
        ("claudemd", "CLAUDE.md"),
        ("claude md", "CLAUDE.md"),
        ("weybar", "waybar"),
        ("vtype", "wtype"),
        ("ghosty", "Ghostty"),
        ("tail net", "tailnet"),
        ("pmpm", "pnpm"),
        // Project-specific
        ("wavevoice", "wayvoice"),
        ("jus", "just"),
        ("whisper flow", "wisprflow"),
        (".files", "dotfiles"),
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

#[cfg(feature = "hud-ui")]
pub fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64)> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0))
}
