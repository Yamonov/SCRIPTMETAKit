#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

command="${1:---smoke}"
if [[ "$#" -gt 0 ]]; then
  shift
fi

case "${command}" in
  --smoke)
    cargo test -p scriptmetakit --test package_smoke imports_scriptmetakit_crate -- --exact "$@"
    ;;
  --test)
    cargo test --workspace --all-targets --all-features "$@"
    ;;
  --clippy)
    cargo clippy --workspace --all-targets --all-features -- -D warnings "$@"
    ;;
  *)
    echo "Usage: $0 [--smoke|--test|--clippy]" >&2
    exit 2
    ;;
esac
