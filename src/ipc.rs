use crate::daemon::Daemon;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

fn socket_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("wayvoice.sock")
}

pub async fn run_server(daemon: Arc<Mutex<Daemon>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = socket_path();
    let _ = tokio::fs::remove_file(&path).await;

    let listener = UnixListener::bind(&path)?;
    println!("Listening on {path:?}");

    loop {
        let (stream, _) = listener.accept().await?;
        let daemon = daemon.clone();
        tokio::spawn(handle_client(stream, daemon));
    }
}

async fn handle_client(stream: UnixStream, daemon: Arc<Mutex<Daemon>>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    if reader.read_line(&mut line).await.is_ok() {
        let response = match line.trim() {
            "toggle" => {
                let mut d = daemon.lock().await;
                d.toggle().await.to_string()
            }
            "cancel" => {
                let mut d = daemon.lock().await;
                d.cancel().await.to_string()
            }
            "status" => {
                let d = daemon.lock().await;
                d.status().to_string()
            }
            _ => "unknown".to_string(),
        };

        let _ = writer.write_all(response.as_bytes()).await;
        let _ = writer.write_all(b"\n").await;
    }
}

pub async fn send_command(cmd: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).await?;

    stream.write_all(cmd.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).await?;

    Ok(response.trim().to_string())
}
