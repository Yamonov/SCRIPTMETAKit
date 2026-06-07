#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "${1:-}" == "--test" ]]; then
  shift
  cargo test --workspace --all-targets --all-features "$@"
  exit 0
fi

if [[ "${1:-}" == "--clippy" ]]; then
  shift
  cargo clippy --workspace --all-targets --all-features -- -D warnings "$@"
  exit 0
fi

if [[ "$#" -eq 0 ]]; then
  set -- --help
fi

cargo run --example scan_folder -- "$@"
