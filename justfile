set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# Show available recipes
default:
    @just --list

# Local development: auto format, test, and build
go: autofmt clippy test build

# Run all CI checks
ci: fmt clippy test

# Check formatting
fmt:
    cargo fmt --all --check

# Auto format all files
autofmt:
    cargo fmt --all

# Run clippy
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Run tests
test:
    cargo nextest run --locked

# Build release binary
build:
    cargo build --release --locked
