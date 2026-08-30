#!/usr/bin/env bash
# The repository gate. Green here is the bar for main.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check-provider-wires.py
python3 scripts/check-app-server-profile.py
python3 scripts/check-cli-contract.py
# Two steps, because a check that passed everything would look exactly like a green one.
python3 scripts/check-no-home-paths.py --self-test
python3 scripts/check-no-home-paths.py
printf 'gate: green\n'
