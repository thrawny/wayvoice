use log::debug;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, value};

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    #[default]
    Groq,
    Codex,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub openai_api_key: String,
    pub groq_api_key: String,
    pub prompt: String,
    pub language: String,
    pub model: String,
    pub keywords: Vec<String>,
    pub extra_keywords: Vec<String>,
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
    pub post_process: bool,
    pub post_process_model: String,
    pub post_process_prompt: String,
    pub post_command: Option<String>,
    pub post_command_timeout: u64,
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
            extra_keywords: Vec::new(),
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
            post_process: false,
            post_process_model: String::new(),
            post_process_prompt: String::new(),
            post_command: None,
            post_command_timeout: 30,
        }
    }
}

#[derive(Debug)]
pub enum ConfigWriteError {
    EmptyReplacementKey,
    EmptyKeyword,
    InvalidReplacementsTable,
    InvalidExtraKeywordsArray,
    ParseToml(toml_edit::TomlError),
    ReadConfig(std::io::Error),
    WriteConfig(std::io::Error),
}

impl fmt::Display for ConfigWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyReplacementKey => write!(f, "replacement key cannot be empty"),
            Self::EmptyKeyword => write!(f, "keyword cannot be empty"),
            Self::InvalidReplacementsTable => {
                write!(f, "`replacements` exists but is not a TOML table")
            }
            Self::InvalidExtraKeywordsArray => {
                write!(f, "`extra_keywords` exists but is not a TOML array")
            }
            Self::ParseToml(err) => write!(f, "failed to parse config TOML: {err}"),
            Self::ReadConfig(err) => write!(f, "failed to read config file: {err}"),
            Self::WriteConfig(err) => write!(f, "failed to write config file: {err}"),
        }
    }
}

impl std::error::Error for ConfigWriteError {}

fn config_base_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|path| path.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    config_base_dir().join("wayvoice").join("config.toml")
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
        "zmx",
        "direnv",
        "pipewire",
        "zathura",
        "symlink",
        "bubblewrap",
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
        ("cmx", "zmx"),
        ("drn", "direnv"),
        ("Satura", "zathura"),
        ("sim link", "symlink"),
        ("sealer", "cli"),
        ("BubbleRub", "bubblewrap"),
        ("~mpm", "npm"),
        ("dove shell", "devshell"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn try_load_config() -> Result<Config, config::ConfigError> {
    try_load_config_at_path(&config_path())
}

fn try_load_config_at_path(path: &Path) -> Result<Config, config::ConfigError> {
    config::Config::builder()
        .add_source(config::File::from(path).required(false))
        .add_source(config::Environment::with_prefix("VOICE"))
        .build()
        .and_then(|c| c.try_deserialize())
        .map(finalize_config)
}

pub fn load_config() -> Config {
    try_load_config().unwrap_or_else(|e| {
        eprintln!("Config error: {e}");
        Config::default()
    })
}

fn finalize_config(mut config: Config) -> Config {
    let keywords = config
        .keywords
        .iter()
        .chain(config.extra_keywords.iter())
        .map(String::as_str)
        .collect::<Vec<_>>();

    if !keywords.is_empty() {
        let kw = keywords.join(", ");
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

pub fn upsert_replacement(from: &str, to: &str) -> Result<PathBuf, ConfigWriteError> {
    let path = config_path();
    upsert_replacement_at_path(&path, from, to)?;
    Ok(path)
}

pub fn append_keyword(keyword: &str) -> Result<PathBuf, ConfigWriteError> {
    let path = config_path();
    append_keyword_at_path(&path, keyword)?;
    Ok(path)
}

fn upsert_replacement_at_path(path: &Path, from: &str, to: &str) -> Result<(), ConfigWriteError> {
    let key = from.trim();
    if key.is_empty() {
        return Err(ConfigWriteError::EmptyReplacementKey);
    }

    let mut doc = load_config_document(path)?;
    ensure_replacements_table(&mut doc)?.insert(key, value(to));
    save_config_document(path, doc)
}

fn append_keyword_at_path(path: &Path, keyword: &str) -> Result<(), ConfigWriteError> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Err(ConfigWriteError::EmptyKeyword);
    }

    let mut doc = load_config_document(path)?;
    let keywords = ensure_extra_keywords_array(&mut doc)?;
    if !keywords.iter().any(|value| value.as_str() == Some(keyword)) {
        keywords.push(keyword);
    }

    save_config_document(path, doc)
}

fn load_config_document(path: &Path) -> Result<DocumentMut, ConfigWriteError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            if contents.trim().is_empty() {
                Ok(DocumentMut::new())
            } else {
                contents
                    .parse::<DocumentMut>()
                    .map_err(ConfigWriteError::ParseToml)
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(err) => Err(ConfigWriteError::ReadConfig(err)),
    }
}

fn save_config_document(path: &Path, doc: DocumentMut) -> Result<(), ConfigWriteError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigWriteError::WriteConfig)?;
    }
    std::fs::write(path, doc.to_string()).map_err(ConfigWriteError::WriteConfig)?;
    Ok(())
}

fn ensure_replacements_table(doc: &mut DocumentMut) -> Result<&mut Table, ConfigWriteError> {
    if !doc.contains_key("replacements") {
        doc["replacements"] = Item::Table(Table::new());
    }

    doc["replacements"]
        .as_table_mut()
        .ok_or(ConfigWriteError::InvalidReplacementsTable)
}

fn ensure_extra_keywords_array(doc: &mut DocumentMut) -> Result<&mut Array, ConfigWriteError> {
    if !doc.contains_key("extra_keywords") {
        doc["extra_keywords"] = value(Array::new());
    }

    doc["extra_keywords"]
        .as_array_mut()
        .ok_or(ConfigWriteError::InvalidExtraKeywordsArray)
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

#[cfg(test)]
mod tests {
    use super::{
        ConfigWriteError, append_keyword_at_path, config_path, finalize_config,
        try_load_config_at_path, upsert_replacement_at_path,
    };
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};
    use toml_edit::{DocumentMut, Value};

    fn temp_config_path(test_name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "wayvoice-{test_name}-{}-{unique}",
                std::process::id()
            ))
            .join("wayvoice")
            .join("config.toml")
    }

    #[test]
    fn creates_replacements_table_in_new_config() {
        let path = temp_config_path("create");

        upsert_replacement_at_path(&path, "hyperland", "Hyprland").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let doc = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(doc["replacements"]["hyperland"].as_str(), Some("Hyprland"));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn preserves_existing_config_when_adding_replacement() {
        let path = temp_config_path("preserve");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "provider = \"groq\"\n# keep this comment\n\n[replacements]\n\"pmpm\" = \"pnpm\"\n",
        )
        .unwrap();

        upsert_replacement_at_path(&path, "ghosty", "Ghostty").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("provider = \"groq\""));
        assert!(contents.contains("# keep this comment"));
        let doc = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(doc["replacements"]["pmpm"].as_str(), Some("pnpm"));
        assert_eq!(doc["replacements"]["ghosty"].as_str(), Some("Ghostty"));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejects_non_table_replacements_section() {
        let path = temp_config_path("reject");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "replacements = \"oops\"\n").unwrap();

        let err = upsert_replacement_at_path(&path, "ghosty", "Ghostty").unwrap_err();
        assert!(matches!(err, ConfigWriteError::InvalidReplacementsTable));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn creates_extra_keywords_array_in_new_config() {
        let path = temp_config_path("keyword-create");

        append_keyword_at_path(&path, "Zen Browser").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let doc = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(doc["extra_keywords"][0].as_str(), Some("Zen Browser"));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn preserves_existing_config_when_adding_keyword() {
        let path = temp_config_path("keyword-preserve");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "provider = \"groq\"\n# keep this comment\n").unwrap();

        append_keyword_at_path(&path, "Zen Browser").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("provider = \"groq\""));
        assert!(contents.contains("# keep this comment"));
        let doc = contents.parse::<DocumentMut>().unwrap();
        assert_eq!(doc["extra_keywords"][0].as_str(), Some("Zen Browser"));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn does_not_duplicate_existing_keyword() {
        let path = temp_config_path("keyword-dedupe");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "extra_keywords = [\"Zen Browser\"]\n").unwrap();

        append_keyword_at_path(&path, "Zen Browser").unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let doc = contents.parse::<DocumentMut>().unwrap();
        let keywords = doc["extra_keywords"].as_array().unwrap();
        assert_eq!(keywords.len(), 1);
        assert_eq!(keywords.get(0).and_then(Value::as_str), Some("Zen Browser"));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejects_non_array_extra_keywords_section() {
        let path = temp_config_path("keyword-reject");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "extra_keywords = \"oops\"\n").unwrap();

        let err = append_keyword_at_path(&path, "Zen Browser").unwrap_err();
        assert!(matches!(err, ConfigWriteError::InvalidExtraKeywordsArray));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn extra_keywords_are_appended_to_prompt() {
        let config = finalize_config(super::Config {
            prompt: "custom".to_string(),
            extra_keywords: vec!["Zen Browser".to_string()],
            ..super::Config::default()
        });

        assert!(config.prompt.contains("custom"));
        assert!(config.prompt.contains("Zen Browser"));
    }

    #[test]
    fn default_replacements_are_merged_with_custom_replacements() {
        let config = finalize_config(super::Config {
            replacements: HashMap::from([("ghosty".to_string(), "Ghost".to_string())]),
            ..super::Config::default()
        });

        assert_eq!(
            config.replacements.get("cmx").map(String::as_str),
            Some("zmx")
        );
        assert_eq!(
            config.replacements.get("ghosty").map(String::as_str),
            Some("Ghost")
        );
    }

    #[test]
    fn try_load_config_merges_default_replacements() {
        let path = temp_config_path("load-merge");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[replacements]\n\"ghosty\" = \"Ghost\"\n\"Wavois\" = \"wayvoice\"\n",
        )
        .unwrap();

        let config = try_load_config_at_path(&path).unwrap();

        assert_eq!(
            config.replacements.get("cmx").map(String::as_str),
            Some("zmx")
        );
        assert_eq!(
            config.replacements.get("ghosty").map(String::as_str),
            Some("Ghost")
        );
        assert_eq!(
            config.replacements.get("Wavois").map(String::as_str),
            Some("wayvoice")
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn config_path_uses_wayvoice_config_toml() {
        assert!(config_path().ends_with("wayvoice/config.toml"));
    }

    #[test]
    fn loads_codex_provider() {
        let path = temp_config_path("provider-codex");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "provider = \"codex\"\n").unwrap();

        let config = try_load_config_at_path(&path).unwrap();
        assert_eq!(config.provider, super::Provider::Codex);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
