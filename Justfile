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

# Watch daemon in a zmx session (rebuild on changes)
watch provider="groq" session="wayvoice":
    zmx attach {{ session }} sh -lc 'RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec -w src -e rs --restart -- cargo run -- serve'

# Watch daemon without zmx (rebuild on changes)
watch-raw provider="groq":
    RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec -w src -e rs --restart -- cargo run -- serve

# Show HUD preview without recording (for UI iteration)
hud-preview:
    cargo run --features hud-ui -- hud-preview

# Install to ~/.cargo/bin
install:
    cargo install --path . --locked --force

# Run clippy
clippy:
    cargo clippy --fix --allow-dirty --allow-staged --release

# Run tests
test:
    cargo test --release

# Format code
fmt:
    cargo fmt
