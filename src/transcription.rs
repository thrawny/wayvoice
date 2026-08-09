mod codex;
mod elevenlabs;
mod openai_compatible;

use crate::config::{Config, Provider};

pub async fn transcribe_audio(
    audio_data: Vec<u8>,
    config: &Config,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match config.provider {
        Provider::Openai | Provider::Groq => {
            openai_compatible::transcribe_audio(audio_data, config).await
        }
        Provider::Codex => codex::transcribe_audio(audio_data, config).await,
        Provider::Elevenlabs => elevenlabs::transcribe_audio(audio_data, config).await,
    }
}
