#!/usr/bin/env bash
# The repository gate. Green here is the bar for main.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
cargo test --workspace --locked
# The one suite that asks all three `Operations` implementations — `LocalOperations`,
# `ConfinedOperations` and `Split` — the same questions, and names the one that answers differently.
# It ran inside the workspace step above; it is named here as its own step for the reason
# `check-no-home-paths.py` is run twice: a suite that was renamed, moved or deleted would stop
# running and the workspace step would stay green, and a missing conformance suite must not look
# exactly like a passing one. Its own two self-tests are the other half of that argument.
cargo test -p b10x-harness-substrate --locked --test conformance
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check-provider-wires.py
python3 scripts/check-app-server-profile.py
# Two steps each, because a check that passed everything would look exactly like a green one.
python3 scripts/check-cli-contract.py --self-test
python3 scripts/check-cli-contract.py
python3 scripts/check-no-home-paths.py --self-test
python3 scripts/check-no-home-paths.py
printf 'gate: green\n'
