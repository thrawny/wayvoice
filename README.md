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

### 2) Make sure runtime tools are installed (Arch)

```bash
sudo pacman -S pipewire wtype wl-clipboard libnotify
```

### 3) Runtime tools on Nix / NixOS

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

Replacements are **additive by default**: your `[replacements]` are merged on top of built-in defaults.

If you want to use only your own replacements, set:

```toml
use_default_replacements = false
```

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
