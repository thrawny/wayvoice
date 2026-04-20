use crate::config::Config;
use log::debug;
use serde::Deserialize;
use serde_json::json;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: CodexTokens,
}

#[derive(Deserialize)]
struct CodexTokens {
    access_token: String,
    account_id: String,
}

#[derive(Debug)]
struct CodexCredentials {
    access_token: String,
    account_id: String,
}

#[derive(Debug)]
enum CodexError {
    AuthFileMissing(PathBuf),
    CredentialsUnavailable,
    InvalidAuthFile(String),
    InvalidResponse,
    RefreshFailed(String),
    RequestFailed(String),
    ServerError(String),
    Unauthorized,
}

impl fmt::Display for CodexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthFileMissing(path) => write!(
                f,
                "Codex auth file not found at {}. Sign in to Codex first.",
                path.display()
            ),
            Self::CredentialsUnavailable => {
                write!(f, "Codex auth is incomplete. Sign in to Codex again.")
            }
            Self::InvalidAuthFile(err) => write!(f, "Failed to parse Codex auth file: {err}"),
            Self::InvalidResponse => write!(f, "Codex returned an invalid transcription response."),
            Self::RefreshFailed(err) => write!(f, "Codex auth refresh failed: {err}"),
            Self::RequestFailed(err) => write!(f, "Codex request failed: {err}"),
            Self::ServerError(err) => write!(f, "Codex transcription failed: {err}"),
            Self::Unauthorized => write!(f, "Codex auth expired. Sign in to Codex again."),
        }
    }
}

impl std::error::Error for CodexError {}

pub async fn transcribe_audio(
    audio_data: Vec<u8>,
    config: &Config,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !config.model.is_empty() || !config.language.is_empty() || !config.prompt.is_empty() {
        debug!("provider=Codex ignores model, language, and prompt settings");
    }

    let credentials = read_codex_credentials()?;
    match perform_codex_transcription(&audio_data, &credentials).await {
        Ok(text) => Ok(text),
        Err(CodexError::Unauthorized) => {
            debug!("provider=Codex unauthorized; refreshing auth");
            refresh_codex_auth().await?;
            let refreshed = read_codex_credentials()?;
            let text = perform_codex_transcription(&audio_data, &refreshed).await?;
            Ok(text)
        }
        Err(err) => Err(err.into()),
    }
}

fn codex_auth_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".codex")
        .join("auth.json")
}

fn read_codex_credentials() -> Result<CodexCredentials, CodexError> {
    read_codex_credentials_from_path(&codex_auth_path())
}

fn read_codex_credentials_from_path(path: &Path) -> Result<CodexCredentials, CodexError> {
    if !path.exists() {
        return Err(CodexError::AuthFileMissing(path.to_path_buf()));
    }

    let data = std::fs::read(path).map_err(|err| CodexError::InvalidAuthFile(err.to_string()))?;
    let auth: CodexAuthFile = serde_json::from_slice(&data)
        .map_err(|err| CodexError::InvalidAuthFile(err.to_string()))?;

    if auth.tokens.access_token.is_empty() || auth.tokens.account_id.is_empty() {
        return Err(CodexError::CredentialsUnavailable);
    }

    Ok(CodexCredentials {
        access_token: auth.tokens.access_token,
        account_id: auth.tokens.account_id,
    })
}

async fn refresh_codex_auth() -> Result<(), CodexError> {
    let program = codex_cli_program();
    let mut child = Command::new(&program)
        .arg("app-server")
        .arg("--listen")
        .arg("stdio://")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            CodexError::RefreshFailed(format!(
                "failed to launch `{program} app-server --listen stdio://`: {err}"
            ))
        })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| CodexError::RefreshFailed("failed to open Codex stdin".to_string()))?;

    for message in [
        json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "wayvoice",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "experimentalApi": true,
                    "optOutNotificationMethods": [],
                },
            },
        }),
        json!({
            "id": 2,
            "method": "account/read",
            "params": {
                "refreshToken": true,
            },
        }),
    ] {
        let line = serde_json::to_vec(&message)
            .map_err(|err| CodexError::RefreshFailed(format!("failed to encode request: {err}")))?;
        stdin
            .write_all(&line)
            .await
            .map_err(|err| CodexError::RefreshFailed(format!("failed to write request: {err}")))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|err| CodexError::RefreshFailed(format!("failed to write newline: {err}")))?;
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .map_err(|err| CodexError::RefreshFailed(format!("failed to wait for Codex: {err}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("Codex exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(CodexError::RefreshFailed(message));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("\"id\":2") || !stdout.contains("\"result\"") {
        return Err(CodexError::RefreshFailed(
            "Codex did not confirm the auth refresh request".to_string(),
        ));
    }

    Ok(())
}

fn codex_cli_program() -> String {
    std::env::var("CODEX_CLI_PATH").unwrap_or_else(|_| "codex".to_string())
}

async fn perform_codex_transcription(
    audio_data: &[u8],
    credentials: &CodexCredentials,
) -> Result<String, CodexError> {
    let boundary = format!(
        "----wayvoice-codex-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let body = make_codex_multipart_body(audio_data, &boundary);
    let endpoint = "https://chatgpt.com/backend-api/transcribe";
    debug!("provider=Codex endpoint={endpoint}");

    let client = reqwest::Client::new();
    let api_start = std::time::Instant::now();
    let response = client
        .post(endpoint)
        .header(
            "Authorization",
            format!("Bearer {}", credentials.access_token),
        )
        .header("ChatGPT-Account-Id", &credentials.account_id)
        .header("originator", "Codex Desktop")
        .header("User-Agent", codex_user_agent())
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("Accept", "application/json")
        .body(body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|err| CodexError::RequestFailed(err.to_string()))?;
    debug!("api_call: {:?}", api_start.elapsed());

    let status = response.status();
    let data = response
        .bytes()
        .await
        .map_err(|err| CodexError::RequestFailed(err.to_string()))?;

    if status.as_u16() == 401 {
        return Err(CodexError::Unauthorized);
    }

    if !status.is_success() {
        let body = String::from_utf8_lossy(&data).trim().to_string();
        return Err(CodexError::ServerError(if body.is_empty() {
            format!("HTTP {status}")
        } else {
            body
        }));
    }

    if let Ok(decoded) = serde_json::from_slice::<TranscriptionResponse>(&data) {
        return Ok(decoded.text.trim().to_string());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&data).map_err(|_| CodexError::InvalidResponse)?;
    let text = value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or(CodexError::InvalidResponse)?;
    Ok(text.trim().to_string())
}

fn make_codex_multipart_body(audio_data: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(audio_data);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

fn codex_user_agent() -> String {
    format!(
        "Codex Desktop/{} (Linux; {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CodexError, codex_user_agent, make_codex_multipart_body, read_codex_credentials_from_path,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("wayvoice-{name}-{unique}.json"))
    }

    #[test]
    fn reads_codex_credentials_from_auth_file() {
        let path = temp_path("codex-auth");
        std::fs::write(
            &path,
            r#"{"tokens":{"access_token":"token-123","account_id":"acct-456"}}"#,
        )
        .unwrap();

        let credentials = read_codex_credentials_from_path(&path).unwrap();
        assert_eq!(credentials.access_token, "token-123");
        assert_eq!(credentials.account_id, "acct-456");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_incomplete_codex_auth_file() {
        let path = temp_path("codex-auth-incomplete");
        std::fs::write(
            &path,
            r#"{"tokens":{"access_token":"","account_id":"acct-456"}}"#,
        )
        .unwrap();

        let err = read_codex_credentials_from_path(&path).unwrap_err();
        assert!(matches!(err, CodexError::CredentialsUnavailable));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn codex_multipart_body_contains_audio_file_part() {
        let body = make_codex_multipart_body(&[1, 2, 3], "boundary-123");
        let body = String::from_utf8_lossy(&body);

        assert!(
            body.contains("Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"")
        );
        assert!(body.contains("Content-Type: audio/wav"));
        assert!(body.contains("--boundary-123--"));
    }

    #[test]
    fn codex_user_agent_looks_like_codex_desktop() {
        let user_agent = codex_user_agent();
        assert!(user_agent.starts_with("Codex Desktop/"));
        assert!(user_agent.contains(std::env::consts::ARCH));
    }
}
