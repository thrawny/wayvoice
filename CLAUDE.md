# wayvoice — Agent Instructions (shared for Claude and other agents)

This file is the behavioral source of truth for agents in this repo.

wayvoice is a Wayland voice-to-text daemon that records audio, transcribes with Whisper APIs (Groq/OpenAI), and injects text into the active app.

## 1) How to execute tasks

- Prefer `just` recipes over raw commands.
- Primary dev loop command is:
  - `just watch` (runs in zmx)

## 2) After code changes

- Run `just check` after every code change (runs fmt, clippy, test).
- If changes affect dev-shell dependencies (`flake.nix` / system libs), tell the human to restart/reload their session so `direnv` picks up the new flake environment.

## 3) Runtime / debugging workflow

- Assume the user wants `just watch` running after development work.
- If asked to inspect runtime behavior, check zmx logs directly.
- Use:
  - `zmx list --short`
  - `zmx history wayvoice | tail -n 200`

## 4) Logging/output conventions

- Prefer runtime logging via `log` macros (`debug!`, `info!`, `warn!`, `error!`).
- Keep `println!` only for intentional CLI output.

## 5) Quick project facts

- Binary subcommands: `serve`, `toggle`, `cancel`, `status`, `once`, `keyword`, `replace`
- Config path: `~/.config/wayvoice/config.toml` (dont read it directly, may contain secrets)
- Watch session name convention: `wayvoice`
