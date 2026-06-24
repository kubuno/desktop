#!/usr/bin/env bash
# Build the Linux .deb (and .rpm) for the kubuno-sync daemon.
#   bash build_deb.sh            # → target/debian/*.deb + target/generate-rpm/*.rpm
#   bash build_deb.sh --install  # idem, puis installe le .deb localement
set -euo pipefail
cd "$(dirname "$0")"

command -v cargo-deb        >/dev/null || cargo install cargo-deb        --locked
command -v cargo-generate-rpm >/dev/null || cargo install cargo-generate-rpm --locked

cargo build --release -p kubuno-sync
cargo deb -p kubuno-sync --no-build
cargo generate-rpm -p crates/kubuno-sync

echo "✓ paquets :"
ls -1 target/debian/*.deb target/generate-rpm/*.rpm 2>/dev/null || true

if [ "${1:-}" = "--install" ]; then
  sudo apt install -y ./target/debian/*.deb
  echo "✓ kubuno-sync installé"
fi
