# wayvoice

Voice-to-text for Wayland.

`wayvoice` records audio from PipeWire, sends it to Whisper (Groq or OpenAI), applies optional text replacements, then inserts the result into your current app.

---

## Requirements

- Linux + Wayland
- `pw-record` (PipeWire)
- `wtype`
- `wl-copy` (for clipboard mode)
- `notify-send`
- A Whisper API key:
  - `GROQ_API_KEY` **or**
  - `OPENAI_API_KEY`

---

## Install

### 1) Build and install with Cargo

```bash
git clone <your-repo-url> wayvoice
cd wayvoice
cargo install --path . --locked
```

This installs the `wayvoice` binary to `~/.cargo/bin/wayvoice`.

### 2) Make sure runtime tools are installed

On Arch-based systems:

```bash
sudo pacman -S pipewire wtype wl-clipboard libnotify
```

On Debian/Ubuntu-based systems (package names may vary):

```bash
sudo apt install pipewire-bin wtype wl-clipboard libnotify-bin
```

---

## Configuration

Config file path:

```text
~/.config/wayvoice.toml
```

Minimal example:

```toml
provider = "groq" # or "openai"
language = "en"

# Option A: store key in config
# groq_api_key = "..."
# openai_api_key = "..."

# Option B (recommended): use env vars
# export GROQ_API_KEY=...
# export OPENAI_API_KEY=...

[replacements]
"hyperland" = "Hyprland"
```

Replacements are **additive**: your `[replacements]` are merged on top of built-in defaults.

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

### One-shot mode (no daemon)

```bash
wayvoice once
```

Records until Enter, transcribes, and prints text to stdout.

---

## Environment variables

- `VOICE_PROVIDER` — override provider (`groq` or `openai`)
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
