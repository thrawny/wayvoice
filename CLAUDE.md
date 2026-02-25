# wayvoice — Agent Instructions (shared for Claude and other agents)

This file is the behavioral source of truth for agents in this repo.

## 1) How to execute tasks

- Prefer `just` recipes over raw commands.
- Primary dev loop command is:
  - `just watch` (runs in zmx)
- Use `just watch-raw` only if explicitly requested.

## 2) After code changes

- Run validation before finishing:
  - `just fmt`
  - `just test`
- Keep edits surgical and scoped to the user request.
- Avoid unrelated refactors.

## 3) Runtime / debugging workflow

- Assume the user wants `just watch` running after development work.
- If asked to inspect runtime behavior, check zmx logs directly.
- Use:
  - `zmx list --short`
  - `zmx history wayvoice | tail -n 200`

## 4) Logging/output conventions

- Prefer runtime logging via `log` macros (`debug!`, `info!`, `warn!`, `error!`).
- Keep `println!` only for intentional CLI output.

## 5) Safety constraints

- Be conservative with transcription filtering/guards to avoid regressions on valid speech.
- Do not change user-facing behavior broadly unless requested.

## 6) Quick project facts (only what agents need)

- Binary subcommands: `serve`, `toggle`, `cancel`, `status`, `once`
- Config path: `~/.config/wayvoice.toml`
- Watch session name convention: `wayvoice`
