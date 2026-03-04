use log::debug;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    #[default]
    Groq,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub openai_api_key: String,
    pub groq_api_key: String,
    pub prompt: String,
    pub language: String,
    pub model: String,
    pub keywords: Vec<String>,
    pub min_words: usize,
    pub use_default_replacements: bool,
    pub replacements: HashMap<String, String>,
    pub hud_color: Option<String>,
    pub hud: bool,
    pub inject_mode: String,
    pub wtype_delay_ms: Option<u64>,
    pub wtype_key_delay_ms: u64,
    pub notify_send: bool,
    pub debug_recordings: bool,
    pub debug_recordings_dir: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::default(),
            openai_api_key: String::new(),
            groq_api_key: String::new(),
            prompt: String::new(),
            language: "en".to_string(),
            model: String::new(),
            keywords: default_keywords(),
            min_words: 3,
            use_default_replacements: true,
            replacements: HashMap::new(),
            hud_color: None,
            hud: true,
            inject_mode: "clipboard".to_string(),
            wtype_delay_ms: None,
            wtype_key_delay_ms: 5,
            notify_send: false,
            debug_recordings: true,
            debug_recordings_dir: None,
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("wayvoice.toml")
}

fn default_keywords() -> Vec<String> {
    [
        "Hyprland",
        "Niri",
        "Neovim",
        "LazyVim",
        "NixOS",
        "Home Manager",
        "Claude Code",
        "CLAUDE.md",
        "waybar",
        "wtype",
        "Ghostty",
        "tailnet",
        "pnpm",
        "wayvoice",
        "Wisprflow",
        "dotfiles",
        "Groq",
        "TOML",
        "openclaw",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn default_replacements() -> HashMap<String, String> {
    [
        ("hyperland", "Hyprland"),
        ("hyper land", "Hyprland"),
        ("neary", "Niri"),
        ("neovim", "Neovim"),
        ("neo vim", "Neovim"),
        ("lazy vim", "LazyVim"),
        ("lazyvim", "LazyVim"),
        ("nix os", "NixOS"),
        ("home manager", "Home Manager"),
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
        ("wavevoice", "wayvoice"),
        ("jus", "just"),
        ("whisper flow", "Wisprflow"),
        ("whisperflow", "Wisprflow"),
        (".files", "dotfiles"),
        ("adjustswitch", "just switch"),
        ("grok", "Groq"),
        ("sembrowser", "zen browser"),
        ("Josswitch", "just switch"),
        ("marked down", "markdown"),
        ("c-lire", "cli"),
        ("tommel", "TOML"),
        ("in the N", "in the end"),
        ("OpenClaude", "openclaw"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn load_config() -> Config {
    let mut config: Config = config::Config::builder()
        .add_source(config::File::from(config_path()).required(false))
        .add_source(config::Environment::with_prefix("VOICE"))
        .build()
        .and_then(|c| c.try_deserialize())
        .unwrap_or_else(|e| {
            eprintln!("Config error: {e}");
            Config::default()
        });

    if !config.keywords.is_empty() {
        let kw = config.keywords.join(", ");
        if config.prompt.is_empty() {
            config.prompt = kw;
        } else {
            config.prompt = format!("{} {kw}", config.prompt);
        }
    }

    if config.use_default_replacements {
        let mut replacements = default_replacements();
        replacements.extend(std::mem::take(&mut config.replacements));
        config.replacements = replacements;
    }

    debug!("provider={:?}", config.provider);
    config
}

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
