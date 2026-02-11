# wayvoice

Voice-to-text daemon for Wayland. Records audio via PipeWire (`pw-record`), transcribes via Whisper API (OpenAI or Groq), applies tech-term replacements, and injects text via `wtype` or clipboard (`wl-copy`).

## Task Runner

```bash
just              # List all recipes
just build        # Build with release profile
just install      # Install to ~/.cargo/bin
just test         # Run tests
just clippy       # Lint
just fmt           # Format
just watch        # Watch + run daemon (default: groq)
just watch openai # Watch + run daemon with OpenAI
```

## Architecture

Single binary with subcommands:

| Command | Description |
|---------|-------------|
| `serve` | Run daemon (Unix socket server, toggle/cancel/status) |
| `toggle` | Toggle recording on/off (sends to daemon) |
| `cancel` | Cancel current operation |
| `status` | Get current state (idle/recording/transcribing) |
| `once` | One-shot: record until Enter, transcribe, print to stdout |

## Source Layout

```
src/
└── main.rs     # Everything: config, daemon, transcription, text injection, CLI
```

## Config

Config file: `~/.config/wayvoice.toml`

```toml
provider = "groq"           # or "openai"
groq_api_key = "..."        # or use GROQ_API_KEY env var
openai_api_key = "..."      # or use OPENAI_API_KEY env var
language = "en"
model = ""                  # default: whisper-large-v3-turbo (groq) or whisper-1 (openai)
prompt = "..."              # context hint for Whisper

[replacements]
"hyperland" = "Hyprland"    # custom text replacements (merged with defaults)
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `VOICE_PROVIDER` | Override provider (groq/openai) |
| `VOICE_INJECT_MODE` | Text injection: "clipboard" (default) or "wtype" |
| `VOICE_WTYPE_DELAY_MS` | Delay before typing (default: 50 clipboard, 100 wtype) |
| `VOICE_WTYPE_KEY_DELAY_MS` | Per-key delay (default: 5) |

## Runtime Dependencies

- `pw-record` (PipeWire) — audio recording
- `wtype` — text injection / paste simulation
- `wl-copy` — clipboard (when using clipboard mode)
- `notify-send` — desktop notifications

## Dev Shell

`flake.nix` provides a dev shell with pkg-config. Activated automatically via `.envrc`.
