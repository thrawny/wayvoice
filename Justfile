# wayvoice task runner

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
    zmx run wayvoice-build 'watchexec -w src -e rs --debounce 5000ms -- cargo build'
    zmx run wayvoice 'RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec --restart --debounce 3000ms -w target/debug/wayvoice -- ./target/debug/wayvoice serve'
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
check:
    just fmt
    cargo clippy -- -D warnings
    just test

# Run tests
test:
    cargo test --release

# Run keyword eval tests (requires GROQ_API_KEY)
eval:
    cargo test --release -- --ignored --nocapture

# Format code
fmt:
    cargo fmt
