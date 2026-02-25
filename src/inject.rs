use crate::config::Config;
use log::debug;
use tokio::process::Command;

pub async fn inject_text(text: &str, config: &Config) {
    if config.inject_mode == "clipboard" {
        inject_via_clipboard(text, config).await;
        return;
    }

    let delay_ms = config.wtype_delay_ms.unwrap_or(100);
    let key_delay_ms = config.wtype_key_delay_ms;
    debug!(
        "wtype delay_ms={delay_ms} key_delay_ms={key_delay_ms} text_len={}",
        text.len()
    );

    let mut cmd = Command::new("wtype");
    if delay_ms > 0 {
        cmd.args(["-s", &delay_ms.to_string()]);
    }
    if key_delay_ms > 0 {
        cmd.args(["-d", &key_delay_ms.to_string()]);
    }
    cmd.arg("--").arg(text);
    let status = cmd.status().await;
    if let Err(e) = status {
        eprintln!("wtype failed: {e}");
        notify("Injection failed", config).await;
    }
}

async fn inject_via_clipboard(text: &str, config: &Config) {
    let delay_ms = config.wtype_delay_ms.unwrap_or(50);
    debug!(
        "injector=clipboard delay_ms={delay_ms} text_len={}",
        text.len()
    );

    let mut copy = Command::new("wl-copy");
    copy.arg("--").arg(text);
    if let Err(e) = copy.status().await {
        eprintln!("wl-copy failed: {e}");
        notify("Injection failed", config).await;
        return;
    }

    if delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }

    let status = Command::new("wtype")
        .args([
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ])
        .status()
        .await;

    if let Err(e) = status {
        eprintln!("wtype failed: {e}");
        notify("Injection failed", config).await;
    }
}

pub async fn notify(message: &str, config: &Config) {
    if !config.notify_send {
        debug!("notify(hud-only): {message}");
        return;
    }

    let _ = Command::new("notify-send")
        .args([
            "--app-name=wayvoice",
            "--expire-time=2000",
            "wayvoice",
            message,
        ])
        .status()
        .await;
}
