use crate::config::{Config, Provider};
use log::debug;
use serde::Deserialize;

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
        default_model(config.provider)
    } else {
        &config.model
    };

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", model.to_string());

    if !config.language.is_empty() {
        form = form.text("language", config.language.clone());
    }

    if !config.prompt.is_empty() {
        form = form.text("prompt", config.prompt.clone());
    }

    let endpoint = api_endpoint(config.provider);
    debug!("provider={:?} endpoint={endpoint}", config.provider);

    let client = reqwest::Client::new();
    let api_start = std::time::Instant::now();
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await?;
    debug!("api_call: {:?}", api_start.elapsed());

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error {status}: {body}").into());
    }

    let result: TranscriptionResponse = response.json().await?;
    Ok(result.text.trim().to_string())
}

fn resolve_api_key(config: &Config) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match config.provider {
        Provider::Openai => {
            if !config.openai_api_key.is_empty() {
                return Ok(config.openai_api_key.clone());
            }
            std::env::var("OPENAI_API_KEY")
                .map_err(|_| "OPENAI_API_KEY not set and no openai_api_key in voice.toml".into())
        }
        Provider::Groq => {
            if !config.groq_api_key.is_empty() {
                return Ok(config.groq_api_key.clone());
            }
            std::env::var("GROQ_API_KEY")
                .map_err(|_| "GROQ_API_KEY not set and no groq_api_key in voice.toml".into())
        }
    }
}

fn api_endpoint(provider: Provider) -> &'static str {
    match provider {
        Provider::Openai => "https://api.openai.com/v1/audio/transcriptions",
        Provider::Groq => "https://api.groq.com/openai/v1/audio/transcriptions",
    }
}

fn default_model(provider: Provider) -> &'static str {
    match provider {
        Provider::Openai => "whisper-1",
        Provider::Groq => "whisper-large-v3-turbo",
    }
}
