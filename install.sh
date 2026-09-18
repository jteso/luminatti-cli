#!/usr/bin/env sh
# Build and install Luminatti for the current user.
#
# Defaults to ~/.local/bin so it never needs elevated privileges. Override
# PREFIX or BIN_DIR when installing into another location, e.g.:
#   PREFIX=/usr/local ./install.sh
#   BIN_DIR="$HOME/bin" ./install.sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin_dir=${BIN_DIR:-"${PREFIX:-"$HOME/.local"}/bin"}
binary_name=luminatti

if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' "error: Cargo is required to build Luminatti. Install Rust from https://rustup.rs/." >&2
    exit 1
fi

printf '%s\n' "Building optimized $binary_name..."
cargo build --release --manifest-path "$script_dir/Cargo.toml" --bin "$binary_name"

if ! mkdir -p "$bin_dir" 2>/dev/null; then
    printf '%s\n' "error: cannot create $bin_dir. Choose a writable location with BIN_DIR=..." >&2
    exit 1
fi

install -m 755 "$script_dir/target/release/$binary_name" "$bin_dir/$binary_name"
printf '%s\n' "Installed $binary_name to $bin_dir/$binary_name"

case ":${PATH}:" in
    *":${bin_dir}:"*) ;;
    *)
        printf '%s\n' "Add this to your shell profile if needed: export PATH=\"$bin_dir:\$PATH\""
        ;;
esac
