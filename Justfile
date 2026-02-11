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

# Watch daemon (rebuild on changes)
watch provider="groq":
    RUST_LOG=debug VOICE_PROVIDER={{ provider }} watchexec -w src -e rs --restart -- cargo run -- serve

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
