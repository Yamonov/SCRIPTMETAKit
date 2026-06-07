#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

FEATURES="${FEATURES:-blocking-http,native-watch}"
PROFILE="${PROFILE:-release}"
OUT_DIR="${ROOT}/Artifacts"
XCFRAMEWORK_PATH="${OUT_DIR}/ScriptMetaKitFFI.xcframework"
BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/scriptmetakit-xcframework.XXXXXX")"
HEADERS_DIR="${BUILD_DIR}/ScriptMetaKitFFIHeaders"
LEGACY_HEADERS_DIR="${OUT_DIR}/ScriptMetaKitFFIHeaders"
trap 'rm -rf "${BUILD_DIR}"' EXIT

if [[ "${PROFILE}" != "release" ]]; then
  echo "Unsupported PROFILE=${PROFILE}. Use PROFILE=release." >&2
  exit 1
fi

export PATH="${HOME}/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:${PATH:-}"

CARGO_BIN="$(command -v cargo || true)"
if [[ -z "${CARGO_BIN}" ]]; then
  echo "cargo was not found. Install Rust or add cargo to PATH." >&2
  exit 1
fi

rm -rf "${XCFRAMEWORK_PATH}" "${LEGACY_HEADERS_DIR}"
mkdir -p "${OUT_DIR}"
mkdir -p "${HEADERS_DIR}" "${BUILD_DIR}"
cp "${ROOT}/scriptmetakit_ffi/include/scriptmetakit_ffi.h" "${HEADERS_DIR}/"
cat > "${HEADERS_DIR}/module.modulemap" <<'MODULEMAP'
module ScriptMetaKitFFI {
  header "scriptmetakit_ffi.h"
  export *
}
MODULEMAP

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  "${CARGO_BIN}" build \
    -p scriptmetakit_ffi \
    --features "${FEATURES}" \
    --release \
    --target "${target}" \
    --manifest-path "${ROOT}/Cargo.toml"

  dylib="${ROOT}/target/${target}/release/libscriptmetakit_ffi.dylib"
  install_name_tool -id "@rpath/libscriptmetakit_ffi.dylib" "${dylib}"
done

UNIVERSAL_DYLIB="${BUILD_DIR}/libscriptmetakit_ffi.dylib"
lipo -create \
  "${ROOT}/target/aarch64-apple-darwin/release/libscriptmetakit_ffi.dylib" \
  "${ROOT}/target/x86_64-apple-darwin/release/libscriptmetakit_ffi.dylib" \
  -output "${UNIVERSAL_DYLIB}"
install_name_tool -id "@rpath/libscriptmetakit_ffi.dylib" "${UNIVERSAL_DYLIB}"

xcodebuild -create-xcframework \
  -library "${UNIVERSAL_DYLIB}" \
  -headers "${HEADERS_DIR}" \
  -output "${XCFRAMEWORK_PATH}"

echo "Created ${XCFRAMEWORK_PATH}"
