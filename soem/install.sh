#!/usr/bin/env bash
# Builds soem and puts it in ~/.cargo/bin:  bash ~/utils/soem/install.sh
set -euo pipefail
cd "$(dirname "$0")"
cargo install --path . --locked
