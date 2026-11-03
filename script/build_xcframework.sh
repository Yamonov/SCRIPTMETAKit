#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

readonly RELEASE_FEATURES="blocking-http,native-watch"
readonly PROFILE="release"
readonly OUT_DIR="${ROOT}/Artifacts"
readonly XCFRAMEWORK_PATH="${OUT_DIR}/ScriptMetaKitFFI.xcframework"
readonly MANIFEST_PATH="${OUT_DIR}/ScriptMetaKitFFI.manifest.json"
readonly SIGNING_IDENTITY="${SCRIPTMETAKIT_SIGNING_IDENTITY:--}"

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
export CARGO_ENCODED_RUSTFLAGS="$(python3 script/build_support.py rust-flags)"
export MACOSX_DEPLOYMENT_TARGET=13.0
# Rust's implicit debuginfo strip can misalign the Mach-O string pool. Keep
# the linker layout intact; the Apple strip below performs the final reduction.
export CARGO_PROFILE_RELEASE_STRIP=none

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
    --locked \
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
if [[ "${SIGNING_IDENTITY}" == "-" ]]; then
  codesign --force --sign - --identifier org.iwashi.SCRIPTMETAKit.ffi "${UNIVERSAL_DYLIB}"
  SIGNATURE_KIND="ad-hoc"
else
  codesign --force --sign "${SIGNING_IDENTITY}" --identifier org.iwashi.SCRIPTMETAKit.ffi \
    --options runtime --timestamp "${UNIVERSAL_DYLIB}"
  SIGNATURE_KIND="identity"
fi
codesign --verify --strict "${UNIVERSAL_DYLIB}"

xcodebuild -create-xcframework \
  -library "${UNIVERSAL_DYLIB}" \
  -headers "${HEADERS_DIR}" \
  -output "${STAGED_XCFRAMEWORK_PATH}"

if [[ "${SIGNING_IDENTITY}" == "-" ]]; then
  codesign --force --sign - --identifier ScriptMetaKitFFI "${STAGED_XCFRAMEWORK_PATH}"
else
  codesign --force --sign "${SIGNING_IDENTITY}" --identifier ScriptMetaKitFFI \
    --timestamp "${STAGED_XCFRAMEWORK_PATH}"
fi
codesign --verify --deep --strict "${STAGED_XCFRAMEWORK_PATH}"

ARCHITECTURES="$(lipo -archs "${UNIVERSAL_DYLIB}")"
if [[ " ${ARCHITECTURES} " != *" arm64 "* || " ${ARCHITECTURES} " != *" x86_64 "* ]]; then
  echo "Unexpected XCFramework architectures: ${ARCHITECTURES}" >&2
  exit 1
fi
cmp \
  "${ROOT}/scriptmetakit_ffi/include/scriptmetakit_ffi.h" \
  "${STAGED_XCFRAMEWORK_PATH}/macos-arm64_x86_64/Headers/scriptmetakit_ffi.h"

python3 "${ROOT}/script/validate_macos_binary.py" \
  "${STAGED_XCFRAMEWORK_PATH}/macos-arm64_x86_64/libscriptmetakit_ffi.dylib"

PACKAGE_VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "${ROOT}/Cargo.toml")"
GIT_REVISION="$(git rev-parse HEAD)"
SOURCE_STATE="clean"
if [[ -n "$(git status --porcelain --untracked-files=normal -- . ':(exclude)Artifacts')" ]]; then
  SOURCE_STATE="dirty"
fi
SOURCE_TREE_SHA256="$(python3 script/build_support.py source-hash)"
DYLIB_SHA256="$(shasum -a 256 "${UNIVERSAL_DYLIB}" | awk '{print $1}')"
cat > "${STAGED_MANIFEST_PATH}" <<MANIFEST
{
  "packageVersion": "${PACKAGE_VERSION}",
  "gitRevision": "${GIT_REVISION}",
  "sourceState": "${SOURCE_STATE}",
  "sourceTreeSHA256": "${SOURCE_TREE_SHA256}",
  "sourceHashFormatVersion": 2,
  "sourceHashScope": "build inputs, source, tests and fixtures, resources and CI; excludes documentation",
  "features": "${RELEASE_FEATURES}",
  "architectures": "${ARCHITECTURES}",
  "signatureKind": "${SIGNATURE_KIND}",
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
