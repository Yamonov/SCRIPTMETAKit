#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
readonly ROOT
readonly CARGO_ABOUT_VERSION="0.9.1"
readonly OUTPUT_PATH="${ROOT}/Sources/ScriptMetaKit/Resources/THIRD_PARTY_LICENSES.txt"
readonly SUMMARY_OUTPUT_PATH="${ROOT}/Sources/ScriptMetaKit/Resources/THIRD_PARTY_LICENSES_SUMMARY.txt"
readonly TEMPLATE_PATH="${ROOT}/THIRD_PARTY_LICENSES.hbs"
readonly SUMMARY_TEMPLATE_PATH="${ROOT}/THIRD_PARTY_LICENSES_SUMMARY.hbs"
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
TEMP_SUMMARY="$(mktemp "${TMPDIR:-/tmp}/scriptmetakit-license-summary.XXXXXX")"
TEMP_SUMMARY_NORMALIZED="$(mktemp "${TMPDIR:-/tmp}/scriptmetakit-normalized-summary.XXXXXX")"
trap 'rm -f "${TEMP_OUTPUT}" "${TEMP_DEPENDENCIES}" "${TEMP_NORMALIZED}" "${TEMP_SUMMARY}" "${TEMP_SUMMARY_NORMALIZED}"' EXIT

generate_from_template() {
  local template_path="$1"
  local output_path="$2"

  "${CARGO_ABOUT_BIN}" generate \
    --all-features \
    --workspace \
    --locked \
    --fail \
    --config "${CONFIG_PATH}" \
    --manifest-path "${ROOT}/Cargo.toml" \
    --output-file "${output_path}" \
    "${template_path}"
}

normalize_text_file() {
  local source_path="$1"
  local output_path="$2"

  LC_ALL=C sed -e 's/\r$//' -e 's/[[:blank:]]*$//' "${source_path}" | awk '
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
  ' > "${output_path}"
}

generate_from_template "${TEMPLATE_PATH}" "${TEMP_DEPENDENCIES}"
generate_from_template "${SUMMARY_TEMPLATE_PATH}" "${TEMP_SUMMARY}"

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

normalize_text_file "${TEMP_OUTPUT}" "${TEMP_NORMALIZED}"
mv "${TEMP_NORMALIZED}" "${TEMP_OUTPUT}"
normalize_text_file "${TEMP_SUMMARY}" "${TEMP_SUMMARY_NORMALIZED}"
mv "${TEMP_SUMMARY_NORMALIZED}" "${TEMP_SUMMARY}"

if [[ "${CHECK_ONLY}" == true ]]; then
  STALE=false
  for generated_and_committed in \
    "${TEMP_OUTPUT}:${OUTPUT_PATH}" \
    "${TEMP_SUMMARY}:${SUMMARY_OUTPUT_PATH}"
  do
    generated_path="${generated_and_committed%%:*}"
    committed_path="${generated_and_committed#*:}"
    if cmp -s "${generated_path}" "${committed_path}"; then
      continue
    fi
    echo "${committed_path} is stale." >&2
    if [[ -f "${committed_path}" ]]; then
      diff -u "${committed_path}" "${generated_path}" || true
    fi
    STALE=true
  done
  if [[ "${STALE}" == true ]]; then
    echo "Regenerate acknowledgement resources with:" >&2
    echo "  ./script/generate_third_party_licenses.sh" >&2
    exit 1
  fi
  echo "Third-party acknowledgement resources are current."
  exit 0
fi

mv "${TEMP_OUTPUT}" "${OUTPUT_PATH}"
mv "${TEMP_SUMMARY}" "${SUMMARY_OUTPUT_PATH}"
rm -f "${TEMP_DEPENDENCIES}"
rm -f "${TEMP_NORMALIZED}"
rm -f "${TEMP_SUMMARY_NORMALIZED}"
trap - EXIT
echo "Generated ${OUTPUT_PATH}"
echo "Generated ${SUMMARY_OUTPUT_PATH}"
