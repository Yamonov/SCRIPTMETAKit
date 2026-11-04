#!/usr/bin/env python3
"""Run public Swift save APIs across CPU, optimization and language modes."""
from pathlib import Path
import argparse
import json
import os
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "tests/SwiftAPIRegression"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--builder", choices=["swiftpm", "xcodebuildmcp"], default="swiftpm")
    parser.add_argument("--output", type=Path, default=ROOT / "build/swift-api-regression/results.json")
    parser.add_argument("--language-modes", nargs="+", choices=["5", "6"], default=["5", "6"])
    args = parser.parse_args()
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    for source in (ROOT / "Sources/ScriptMetaKit").glob("*.swift"):
        if re.search(r"@_(?:silgen_name|extern)\s*\(", source.read_text()):
            raise RuntimeError(f"Use declarations imported from the C module: {source}")
    version = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M)[1]
    results = []
    for language in args.language_modes:
        environment = dict(os.environ, SMK_TEST_SWIFT_LANGUAGE_MODE=language,
                           MACOSX_DEPLOYMENT_TARGET="13.0")
        for configuration in ["debug", "release"]:
            if args.builder == "xcodebuildmcp":
                command = ["xcodebuildmcp", "swift-package", "build", "--package-path", str(PACKAGE),
                           "--configuration", configuration, "--architectures", "arm64", "x86_64"]
                binary = PACKAGE / ".build/out/Products" / configuration.capitalize() / "SwiftAPIRegression"
            else:
                command = ["swift", "build", "--package-path", str(PACKAGE), "--configuration", configuration,
                           "--arch", "arm64", "--arch", "x86_64"]
                bin_path = subprocess.check_output(command + ["--show-bin-path"], cwd=ROOT, env=environment, text=True).strip()
                binary = Path(bin_path) / "SwiftAPIRegression"
            build = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True)
            log = output.parent / f"build-swift{language}-{configuration}.log"
            log.write_bytes(build.stdout + build.stderr)
            if build.returncode:
                raise RuntimeError(f"Swift {language} {configuration} build failed; see {log}")
            architectures = subprocess.check_output(["lipo", "-archs", str(binary)], text=True).split()
            if not {"arm64", "x86_64"}.issubset(architectures):
                raise RuntimeError(f"The regression executable is not Universal: {architectures}")
            for architecture in ["arm64", "x86_64"]:
                run = subprocess.run(["/usr/bin/arch", "-" + architecture, str(binary), version],
                                     cwd=ROOT, env=environment, capture_output=True, timeout=60)
                run_log = output.parent / f"run-swift{language}-{configuration}-{architecture}.log"
                run_log.write_bytes(run.stdout + run.stderr)
                if run.returncode:
                    raise RuntimeError(f"Swift {language} {configuration} {architecture} failed ({run.returncode}); see {run_log}")
                report = json.loads(run.stdout)
                if (report["architecture"], report["swiftLanguageMode"], report["version"], report["status"]) != (
                    architecture, language, version, "passed"
                ):
                    raise RuntimeError(f"Unexpected regression report: {report}")
                results.append(dict(report, configuration=configuration))
                output.write_text(json.dumps({"version": version, "results": results}, indent=2) + "\n")
                print(f"Swift {language} / {configuration} / {architecture}: passed", flush=True)
    print(f"Public Swift API regression matrix passed: {len(results)} cases", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
