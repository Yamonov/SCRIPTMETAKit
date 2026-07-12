#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
readonly ROOT
readonly CARGO_ABOUT_VERSION="0.9.1"
readonly OUTPUT_PATH="${ROOT}/Sources/ScriptMetaKit/Resources/THIRD_PARTY_LICENSES.txt"
readonly TEMPLATE_PATH="${ROOT}/THIRD_PARTY_LICENSES.hbs"
readonly CONFIG_PATH="${ROOT}/about.toml"
readonly LICENSE_PATH="${ROOT}/LICENSE"
readonly NOTICE_PATH="${ROOT}/NOTICE"

usage() {
  echo "Usage: $0 [--check]" >&2
}

CHECK_ONLY=false
case "${1:-}" in
  "") ;;
  --check) CHECK_ONLY=true ;;
  *)
    usage
    exit 2
    ;;
esac

CARGO_ABOUT_BIN="${CARGO_ABOUT_BIN:-$(command -v cargo-about || true)}"
if [[ -z "${CARGO_ABOUT_BIN}" ]]; then
  echo "cargo-about ${CARGO_ABOUT_VERSION} is required." >&2
  echo "Install it with: cargo install --locked --features cli --version ${CARGO_ABOUT_VERSION} cargo-about" >&2
  exit 1
fi

ACTUAL_VERSION="$(${CARGO_ABOUT_BIN} --version | awk '{print $2}')"
if [[ "${ACTUAL_VERSION}" != "${CARGO_ABOUT_VERSION}" ]]; then
  echo "Expected cargo-about ${CARGO_ABOUT_VERSION}, found ${ACTUAL_VERSION}." >&2
  exit 1
fi

mkdir -p "$(dirname "${OUTPUT_PATH}")"
TEMP_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/scriptmetakit-third-party-licenses.XXXXXX")"
TEMP_DEPENDENCIES="$(mktemp "${TMPDIR:-/tmp}/scriptmetakit-rust-licenses.XXXXXX")"
TEMP_NORMALIZED="$(mktemp "${TMPDIR:-/tmp}/scriptmetakit-normalized-licenses.XXXXXX")"
trap 'rm -f "${TEMP_OUTPUT}" "${TEMP_DEPENDENCIES}" "${TEMP_NORMALIZED}"' EXIT

"${CARGO_ABOUT_BIN}" generate \
  --all-features \
  --workspace \
  --locked \
  --fail \
  --config "${CONFIG_PATH}" \
  --manifest-path "${ROOT}/Cargo.toml" \
  --output-file "${TEMP_DEPENDENCIES}" \
  "${TEMPLATE_PATH}"

{
  echo "SCRIPTMETAKit Acknowledgements"
  echo "=============================="
  echo
  echo "This document contains the SCRIPTMETAKit license and the licenses of the"
  echo "Rust crates included in the macOS release build."
  echo
  echo "SCRIPTMETAKit"
  echo "--------------"
  echo
  cat "${LICENSE_PATH}"
  echo
  echo "NOTICE"
  echo "------"
  echo
  cat "${NOTICE_PATH}"
  echo
  cat "${TEMP_DEPENDENCIES}"
} > "${TEMP_OUTPUT}"

LC_ALL=C sed -e 's/\r$//' -e 's/[[:blank:]]*$//' "${TEMP_OUTPUT}" | awk '
  {
    lines[NR] = $0
  }
  END {
    last = NR
    while (last > 0 && lines[last] == "") {
      last--
    }
    for (line = 1; line <= last; line++) {
      print lines[line]
    }
  }
' > "${TEMP_NORMALIZED}"
mv "${TEMP_NORMALIZED}" "${TEMP_OUTPUT}"

if [[ "${CHECK_ONLY}" == true ]]; then
  if ! cmp -s "${TEMP_OUTPUT}" "${OUTPUT_PATH}"; then
    echo "${OUTPUT_PATH} is stale. Regenerate it with:" >&2
    echo "  script/generate_third_party_licenses.sh" >&2
    diff -u "${OUTPUT_PATH}" "${TEMP_OUTPUT}" || true
    exit 1
  fi
  echo "Third-party licenses are current."
  exit 0
fi

mv "${TEMP_OUTPUT}" "${OUTPUT_PATH}"
rm -f "${TEMP_DEPENDENCIES}"
rm -f "${TEMP_NORMALIZED}"
trap - EXIT
echo "Generated ${OUTPUT_PATH}"
