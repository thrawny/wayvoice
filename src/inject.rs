use log::debug;
use tokio::process::Command;

pub async fn inject_text(text: &str) {
    let mode = injection_mode();
    if mode == "clipboard" {
        inject_via_clipboard(text).await;
        return;
    }

    let delay_ms = wtype_delay_ms(&mode);
    let key_delay_ms = wtype_key_delay_ms();
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
        notify("Injection failed").await;
    }
}

async fn inject_via_clipboard(text: &str) {
    let delay_ms = wtype_delay_ms("clipboard");
    debug!(
        "injector=clipboard delay_ms={delay_ms} text_len={}",
        text.len()
    );

    // Copy to regular clipboard (not primary) for universal compatibility
    let mut copy = Command::new("wl-copy");
    copy.arg("--").arg(text);
    if let Err(e) = copy.status().await {
        eprintln!("wl-copy failed: {e}");
        notify("Injection failed").await;
        return;
    }

    if delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }

    // Use Ctrl+Shift+V to paste (works universally without conflicting with
    // Ghostty's Ctrl+V image paste or requiring xremap translation)
    let status = Command::new("wtype")
        .args([
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ])
        .status()
        .await;

    if let Err(e) = status {
        eprintln!("wtype failed: {e}");
        notify("Injection failed").await;
    }
}

pub async fn notify(message: &str) {
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

fn wtype_delay_ms(mode: &str) -> u64 {
    std::env::var("VOICE_WTYPE_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| if mode == "clipboard" { 50 } else { 100 })
}

fn wtype_key_delay_ms() -> u64 {
    std::env::var("VOICE_WTYPE_KEY_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5)
}

fn injection_mode() -> String {
    std::env::var("VOICE_INJECT_MODE")
        .ok()
        .unwrap_or_else(|| "clipboard".to_string())
}
