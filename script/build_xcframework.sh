#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

readonly RELEASE_FEATURES="blocking-http,native-watch"
readonly PROFILE="release"
readonly OUT_DIR="${ROOT}/Artifacts"
readonly XCFRAMEWORK_PATH="${OUT_DIR}/ScriptMetaKitFFI.xcframework"
readonly MANIFEST_PATH="${OUT_DIR}/ScriptMetaKitFFI.manifest.json"

mkdir -p "${OUT_DIR}"
BUILD_DIR="$(mktemp -d "${OUT_DIR}/.scriptmetakit-xcframework.XXXXXX")"
readonly BUILD_DIR
readonly HEADERS_DIR="${BUILD_DIR}/ScriptMetaKitFFIHeaders"
readonly STAGED_XCFRAMEWORK_PATH="${BUILD_DIR}/ScriptMetaKitFFI.xcframework"
readonly STAGED_MANIFEST_PATH="${BUILD_DIR}/ScriptMetaKitFFI.manifest.json"
trap 'rm -rf "${BUILD_DIR}"' EXIT

if [[ -n "${FEATURES:-}" && "${FEATURES}" != "${RELEASE_FEATURES}" ]]; then
  echo "Release features are fixed to ${RELEASE_FEATURES}; FEATURES=${FEATURES} is not supported." >&2
  exit 1
fi

export PATH="${HOME}/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:${PATH:-}"
export RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }--remap-path-prefix=${ROOT}=. --remap-path-prefix=${HOME}=~"

CARGO_BIN="$(command -v cargo || true)"
if [[ -z "${CARGO_BIN}" ]]; then
  echo "cargo was not found. Install Rust or add cargo to PATH." >&2
  exit 1
fi

mkdir -p "${HEADERS_DIR}"
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
    --features "${RELEASE_FEATURES}" \
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
strip -S -x "${UNIVERSAL_DYLIB}"

xcodebuild -create-xcframework \
  -library "${UNIVERSAL_DYLIB}" \
  -headers "${HEADERS_DIR}" \
  -output "${STAGED_XCFRAMEWORK_PATH}"

ARCHITECTURES="$(lipo -archs "${UNIVERSAL_DYLIB}")"
if [[ " ${ARCHITECTURES} " != *" arm64 "* || " ${ARCHITECTURES} " != *" x86_64 "* ]]; then
  echo "Unexpected XCFramework architectures: ${ARCHITECTURES}" >&2
  exit 1
fi
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_create_default$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_resolve_registered_path$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_write_script_metadata_file_if_unchanged$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_scan_result_file_list_directory_state_ranges$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_set_operational_policy$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_start_watching_with_callback$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_start_watching_with_callback_v2$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_scan_result_root_revisions$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_scan_result_file_list_details$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_scan_result_watch_delivery_info$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_preflight_root$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_load_cache_file_with_limit$'
nm -gU "${UNIVERSAL_DYLIB}" | grep -q '_smk_engine_save_cache_file_with_limit$'
cmp \
  "${ROOT}/scriptmetakit_ffi/include/scriptmetakit_ffi.h" \
  "${STAGED_XCFRAMEWORK_PATH}/macos-arm64_x86_64/Headers/scriptmetakit_ffi.h"

PACKAGE_VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "${ROOT}/Cargo.toml")"
GIT_REVISION="$(git rev-parse HEAD)"
SOURCE_STATE="clean"
if [[ -n "$(git status --porcelain --untracked-files=normal -- . ':(exclude)Artifacts')" ]]; then
  SOURCE_STATE="dirty"
fi
SOURCE_TREE_SHA256="$({
  git ls-files --cached --others --exclude-standard \
    | grep -Ev '^(Artifacts/|target/|\.build/)' \
    | LC_ALL=C sort \
    | while IFS= read -r source_path; do
        shasum -a 256 "${source_path}"
      done
} | shasum -a 256 | awk '{print $1}')"
DYLIB_SHA256="$(shasum -a 256 "${UNIVERSAL_DYLIB}" | awk '{print $1}')"
cat > "${STAGED_MANIFEST_PATH}" <<MANIFEST
{
  "packageVersion": "${PACKAGE_VERSION}",
  "gitRevision": "${GIT_REVISION}",
  "sourceState": "${SOURCE_STATE}",
  "sourceTreeSHA256": "${SOURCE_TREE_SHA256}",
  "features": "${RELEASE_FEATURES}",
  "architectures": "${ARCHITECTURES}",
  "dylibSHA256": "${DYLIB_SHA256}"
}
MANIFEST

PREVIOUS_XCFRAMEWORK_PATH="${OUT_DIR}/.ScriptMetaKitFFI.xcframework.previous.$$"
PREVIOUS_MANIFEST_PATH="${OUT_DIR}/.ScriptMetaKitFFI.manifest.previous.$$"
if [[ -e "${XCFRAMEWORK_PATH}" ]]; then
  mv "${XCFRAMEWORK_PATH}" "${PREVIOUS_XCFRAMEWORK_PATH}"
fi
if [[ -e "${MANIFEST_PATH}" ]]; then
  mv "${MANIFEST_PATH}" "${PREVIOUS_MANIFEST_PATH}"
fi

if ! mv "${STAGED_XCFRAMEWORK_PATH}" "${XCFRAMEWORK_PATH}" \
  || ! mv "${STAGED_MANIFEST_PATH}" "${MANIFEST_PATH}"; then
  rm -rf "${XCFRAMEWORK_PATH}"
  rm -f "${MANIFEST_PATH}"
  if [[ -e "${PREVIOUS_XCFRAMEWORK_PATH}" ]]; then
    mv "${PREVIOUS_XCFRAMEWORK_PATH}" "${XCFRAMEWORK_PATH}"
  fi
  if [[ -e "${PREVIOUS_MANIFEST_PATH}" ]]; then
    mv "${PREVIOUS_MANIFEST_PATH}" "${MANIFEST_PATH}"
  fi
  exit 1
fi

rm -rf "${PREVIOUS_XCFRAMEWORK_PATH}"
rm -f "${PREVIOUS_MANIFEST_PATH}"

echo "Created ${XCFRAMEWORK_PATH}"
echo "Manifest ${MANIFEST_PATH}"
