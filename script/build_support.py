"""Shared reproducible build settings and source fingerprinting for Kit artifacts."""
from pathlib import Path
import hashlib
import os
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def source_tree_sha256(root=ROOT):
    paths = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root
    ).decode().split("\0")
    digest = hashlib.sha256()
    for name in sorted(set(paths) - {""}):
        path = root / name
        included = (
            path.suffix in {".rs", ".swift", ".h", ".c", ".m", ".mm", ".toml", ".py", ".sh", ".ps1"}
            or name == "Cargo.lock"
            or name.startswith(("Sources/ScriptMetaKit/Resources/", ".github/workflows/"))
            or (name.startswith(("tests/", "examples/")) and path.suffix != ".md")
        )
        if included and path.is_file() and not name.startswith(("Artifacts/", "build/", "target/", ".build/")):
            encoded_name = name.encode()
            digest.update(len(encoded_name).to_bytes(8, "big"))
            digest.update(encoded_name)
            digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def encoded_rust_flags(root=ROOT):
    existing = os.environ.get("CARGO_ENCODED_RUSTFLAGS")
    flags = existing.split("\x1f") if existing else shlex.split(os.environ.get("RUSTFLAGS", ""))
    flags.extend([f"--remap-path-prefix={root}=.", f"--remap-path-prefix={Path.home()}=~"])
    return "\x1f".join(flags)


def build_environment(root=ROOT):
    return dict(os.environ, CARGO_ENCODED_RUSTFLAGS=encoded_rust_flags(root),
                CARGO_PROFILE_RELEASE_STRIP="none", MACOSX_DEPLOYMENT_TARGET="13.0")


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) == 2 else ""
    if command == "source-hash":
        print(source_tree_sha256())
    elif command == "rust-flags":
        print(encoded_rust_flags())
    else:
        raise SystemExit("Usage: build_support.py source-hash|rust-flags")
