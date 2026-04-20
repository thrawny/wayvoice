# wayvoice

Voice-to-text for Wayland.

`wayvoice` records audio from PipeWire, sends it to Whisper (Groq or OpenAI) or Codex transcription, applies optional text replacements, then inserts the result into your current app.

---

## Requirements

- Linux + Wayland
- `pw-record` (PipeWire)
- `wtype`
- `wl-copy` (for clipboard mode)
- `notify-send`
- One transcription backend:
  - `GROQ_API_KEY`, or
  - `OPENAI_API_KEY`, or
  - a signed-in Codex CLI with `~/.codex/auth.json`

---

## Install

### 1) Install with Nix flakes

From this repo:

```bash
nix profile install .#wayvoice
```

From GitHub (replace `OWNER/REPO`):

```bash
nix profile install github:OWNER/REPO#wayvoice
```

### 2) Build and install with Cargo

Install:

```bash
cargo install --path . --locked
```

The binary always includes the HUD integration and links against GTK4 + gtk4-layer-shell.

### 3) HUD dependencies

wayvoice links against GTK4 and gtk4-layer-shell. Install the dev packages before building:

**Arch:**

```bash
sudo pacman -S gtk4 gtk4-layer-shell
```

**Nix / NixOS:**

If you use the project's `flake.nix` dev shell, these are already included.

### 4) Runtime tools (Arch)

```bash
sudo pacman -S pipewire wtype wl-clipboard libnotify
```

### 5) Runtime tools on Nix / NixOS

If you installed `wayvoice` from this flake, these tools are already on PATH via wrapper.

If you built with Cargo instead, add these packages manually.

On NixOS, add these packages to your system or Home Manager config:

- `pipewire`
- `wtype`
- `wl-clipboard`
- `libnotify`

On non-NixOS with the Nix package manager:

```bash
nix profile install nixpkgs#pipewire nixpkgs#wtype nixpkgs#wl-clipboard nixpkgs#libnotify
```

---

## Configuration

Config file path:

```text
~/.config/wayvoice/config.toml
```

Minimal example:

```toml
provider = "groq" # or "openai" or "codex"
language = "en"

# Option A: store key in config
# groq_api_key = "..."
# openai_api_key = "..."

# Option B (recommended): use env vars
# export GROQ_API_KEY=...
# export OPENAI_API_KEY=...

# Codex provider uses your local Codex login from ~/.codex/auth.json.
# If the token is stale, wayvoice will try: codex app-server --listen stdio://

# Extra prompt keywords appended after the built-in defaults
# extra_keywords = ["Zen Browser"]

# HUD recording color (hex, default: #fc618d)
# hud_color = "#fc618d"

[replacements]
"hyperland" = "Hyprland"
```

Keywords bias the transcription model upstream via the API prompt. Replacements rewrite the transcript downstream after transcription.

> [!NOTE]
> The `codex` provider is experimental. It ignores `language`, `prompt`, and `model`, because the current Codex transcription endpoint does not expose the same controls as the OpenAI-compatible APIs.

Replacements are **additive by default**: your `[replacements]` are merged on top of built-in defaults.

If you want to use only your own replacements, set:

```toml
use_default_replacements = false
```

You can also add replacements from the CLI:

```bash
wayvoice keyword add "Zen Browser"
wayvoice replace add "hyperland" "Hyprland"
wayvoice replace add --substring "mpm" "npm"
```

When the daemon is running, config updates are reloaded automatically.

---

## Usage

### Run daemon

```bash
wayvoice serve
```

In another terminal (or keybindings):

```bash
wayvoice toggle  # start recording
wayvoice toggle  # stop + transcribe + inject text
wayvoice cancel  # cancel current operation
wayvoice status  # idle / recording / transcribing
```

### xremap keybind (toggle style)

If you use xremap, bind a key to launch `wayvoice toggle` on key press.

```yaml
modmap:
  - name: Wayvoice
    remap:
      Shift_R:
        skip_key_event: true
        press:
          - launch: [wayvoice, toggle]
```

Notes:
- `skip_key_event = true` prevents the original key from being sent.
- This is **toggle style** (press once to start, press again to stop).
- Make sure `wayvoice serve` is running (for example as a user service).

If you prefer **hold-to-record**, trigger `wayvoice toggle` on both `press` and `release`.

### HUD overlay

The daemon automatically spawns a layer-shell HUD that shows recording (pink waveform) and transcribing (blue waveform) states.

The HUD is enabled by default. Disable it with:

```bash
VOICE_HUD=0 wayvoice serve
```

Preview the HUD without running the daemon:

```bash
wayvoice hud-preview    # or: just hud-preview
```

### One-shot mode (no daemon)

```bash
wayvoice once
```

Records until Enter, transcribes, and prints text to stdout.

---

## Environment variables

- `VOICE_PROVIDER` — override provider (`groq`, `openai`, or `codex`)
- `VOICE_HUD` — `0` / `false` / `off` to disable the HUD overlay
- `VOICE_INJECT_MODE` — `clipboard` (default) or `wtype`
- `VOICE_WTYPE_DELAY_MS` — delay before paste/type
- `VOICE_WTYPE_KEY_DELAY_MS` — per-key delay for `wtype`

---

## Development

```bash
just            # show tasks
just build
just test
just fmt
just clippy
just watch      # run daemon with auto-reload
```

If you use Nix + direnv, entering the repo activates the dev shell from `flake.nix`.
