# wayvoice task runner

# Default recipe
default:
    @just --list

# Build
build:
    cargo build --release

# Run the daemon (with HUD support)
run:
    cargo run --features hud-ui -- serve

# Watch daemon in a zmx session (rebuild on changes, with HUD support)
watch provider="groq" session="wayvoice":
    zmx attach {{ session }} sh -lc 'RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec -w src -e rs --restart -- cargo run --features hud-ui -- serve'

# Watch daemon without zmx (rebuild on changes, with HUD support)
watch-raw provider="groq":
    RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec -w src -e rs --restart -- cargo run --features hud-ui -- serve

# Show HUD preview without recording (for UI iteration)
hud-preview:
    cargo run --features hud-ui -- hud-preview

# Install to ~/.cargo/bin
install:
    cargo install --features hud-ui --path . --locked --force

# Run clippy (auto-fix)
clippy:
    cargo clippy --fix --allow-dirty --allow-staged --release

# Run all post-change checks
check:
    just fmt
    cargo clippy --features hud-ui -- -D warnings
    just test

# Run tests
test:
    cargo test --release

# Format code
fmt:
    cargo fmt
