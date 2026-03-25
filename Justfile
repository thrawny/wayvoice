# wayvoice task runner

_build_stamp := "target/debug/wayvoice-built.stamp"

# Default recipe
default:
    @just --list

# Build
build:
    cargo build --release

# Run the daemon
run:
    cargo run -- serve

# Watch daemon with build-gated restart (old process stays alive on compile errors)
watch provider="groq":
    cargo build
    touch {{ _build_stamp }}
    zmx run wayvoice-build watchexec -w src -w Cargo.toml -e rs --debounce 5s --on-busy-update queue -- 'cargo build && touch {{ _build_stamp }}'
    zmx run wayvoice env RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec --restart --debounce 250ms -w {{ _build_stamp }} -- ./target/debug/wayvoice serve
    zmx attach wayvoice

# Show HUD preview without recording (for UI iteration)
hud-preview:
    cargo run -- hud-preview

# Install to ~/.cargo/bin
install:
    cargo install --path . --locked --force

# Run clippy (auto-fix)
clippy:
    cargo clippy --fix --allow-dirty --allow-staged --release

# Run all post-change checks
check: fmt _clippy-strict test

# Clippy with denied warnings (for CI/check)
_clippy-strict:
    cargo clippy -- -D warnings

# Run tests
test:
    cargo test --release

# Run keyword eval tests (requires GROQ_API_KEY)
eval:
    cargo test --release -- --ignored --nocapture

# Format code
fmt:
    cargo fmt
