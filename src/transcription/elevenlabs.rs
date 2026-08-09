use crate::config::Config;
use log::debug;
use serde::Deserialize;

const DEFAULT_MODEL: &str = "scribe_v2";
const ENDPOINT: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const MAX_KEYTERMS: usize = 1000;

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub async fn transcribe_audio(
    audio_data: Vec<u8>,
    config: &Config,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let api_key = resolve_api_key(config)?;

    let file_part = reqwest::multipart::Part::bytes(audio_data)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;

    let model = if config.model.is_empty() {
        DEFAULT_MODEL
    } else {
        &config.model
    };

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model_id", model.to_string())
        .text("tag_audio_events", "false")
        .text("timestamps_granularity", "none")
        .text("no_verbatim", "true");

    if !config.language.is_empty() {
        form = form.text("language_code", config.language.clone());
    }

    let keyterms = keyterms(config);
    if !keyterms.is_empty() {
        debug!("elevenlabs_keyterms={}", keyterms.len());
        for keyterm in keyterms {
            form = form.text("keyterms", keyterm);
        }
    }

    let client = reqwest::Client::new();
    let api_start = std::time::Instant::now();
    let response = client
        .post(ENDPOINT)
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .await?;
    debug!("api_call: {:?}", api_start.elapsed());

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs API error {status}: {body}").into());
    }

    let result: TranscriptionResponse = response.json().await?;
    Ok(result.text.trim().to_string())
}

fn resolve_api_key(config: &Config) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !config.elevenlabs_api_key.is_empty() {
        return Ok(config.elevenlabs_api_key.clone());
    }

    std::env::var("ELEVENLABS_API_KEY")
        .or_else(|_| std::env::var("ELEVEN_LABS_API_KEY"))
        .map_err(|_| {
            "ELEVENLABS_API_KEY/ELEVEN_LABS_API_KEY not set and no elevenlabs_api_key in voice.toml"
                .into()
        })
}

fn keyterms(config: &Config) -> Vec<String> {
    let terms = config
        .keywords
        .iter()
        .filter(|_| config.use_default_keywords)
        .chain(config.extra_keywords.iter())
        .filter_map(|term| normalize_keyterm(term))
        .fold(Vec::<String>::new(), |mut terms, term| {
            if !terms
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&term))
            {
                terms.push(term);
            }
            terms
        });

    terms.into_iter().take(MAX_KEYTERMS).collect()
}

fn normalize_keyterm(term: &str) -> Option<String> {
    let term = term.trim();
    if term.is_empty()
        || term.chars().count() >= 50
        || term.split_whitespace().count() > 5
        || term
            .chars()
            .any(|c| matches!(c, '<' | '>' | '{' | '}' | '[' | ']' | '\\'))
    {
        return None;
    }

    Some(term.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn keyterms_filters_limits_and_dedupes() {
        let config = Config {
            keywords: vec![
                "wayvoice".to_string(),
                "Wayvoice".to_string(),
                "too many words in this one term".to_string(),
                "bad[key]".to_string(),
            ],
            extra_keywords: vec!["Groq".to_string()],
            ..Config::default()
        };

        assert_eq!(keyterms(&config), vec!["wayvoice", "Groq"]);
    }

    #[test]
    fn keyterms_can_skip_defaults() {
        let config = Config {
            use_default_keywords: false,
            keywords: vec!["wayvoice".to_string()],
            extra_keywords: vec!["Groq".to_string()],
            ..Config::default()
        };

        assert_eq!(keyterms(&config), vec!["Groq"]);
    }
}
