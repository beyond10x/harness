#!/usr/bin/env bash
# Compatibility only; CI and AGENTS.md invoke the Rust gate directly. Remove after 0.6.0.
exec cargo xtask gate "$@"
