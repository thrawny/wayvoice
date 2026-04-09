use log::{info, warn};
use serde::Deserialize;
use std::time::Duration;

use crate::config::Config;

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

pub async fn run_post_command(
    text: &str,
    config: &Config,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !config.post_process {
        return Ok(text.to_string());
    }

    let api_key = resolve_api_key(config)?;
    let model = if config.post_process_model.is_empty() {
        "llama-3.3-70b-versatile"
    } else {
        &config.post_process_model
    };

    let prompt = if config.post_process_prompt.is_empty() {
        "Fix any transcription errors in this voice-dictated text. Output only the corrected text with no explanation."
    } else {
        &config.post_process_prompt
    };

    let keywords: Vec<&str> = config
        .keywords
        .iter()
        .chain(config.extra_keywords.iter())
        .map(String::as_str)
        .collect();

    let system_prompt = if keywords.is_empty() {
        prompt.to_string()
    } else {
        format!(
            "{prompt}\n\nPreserve these terms exactly as spelled: {}",
            keywords.join(", ")
        )
    };
    info!("post_process: model={model}");

    let start = std::time::Instant::now();
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(&api_key)
        .json(&serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": text},
            ],
        }))
        .timeout(Duration::from_secs(config.post_command_timeout))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("post_process API error {status}: {body}").into());
    }

    let result: ChatResponse = response.json().await?;
    let output = result
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .unwrap_or_default();

    if output.is_empty() {
        warn!("post_process returned empty, using original text");
        return Ok(text.to_string());
    }

    info!("post_process completed in {:?}: {output}", start.elapsed());
    Ok(output)
}

fn resolve_api_key(config: &Config) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !config.groq_api_key.is_empty() {
        return Ok(config.groq_api_key.clone());
    }
    std::env::var("GROQ_API_KEY")
        .map_err(|_| "GROQ_API_KEY not set and no groq_api_key in config for post_process".into())
}
