#!/usr/bin/env python3
"""Validate the Universal dylib's string pools, link both slices, and run available CPUs."""

import argparse
from pathlib import Path
import re
import struct
import subprocess
import tempfile


ARCHITECTURES = {0x01000007: "x86_64", 0x0100000C: "arm64"}


def validate_string_pools(library):
    data = library.read_bytes()
    magic, count = struct.unpack_from(">II", data)
    if magic not in (0xCAFEBABE, 0xCAFEBABF):
        raise ValueError("expected a Universal Mach-O dylib")
    entry_format = ">IIIII" if magic == 0xCAFEBABE else ">IIQQII"
    entry_size = struct.calcsize(entry_format)
    found = set()
    for index in range(count):
        cpu, _, start, size, *_ = struct.unpack_from(entry_format, data, 8 + index * entry_size)
        architecture = ARCHITECTURES[cpu]
        if start + size > len(data):
            raise ValueError(f"{architecture}: slice extends beyond the file")
        header_magic, header_cpu, _, file_type, command_count, *_ = struct.unpack_from(
            "<8I", data, start
        )
        if (header_magic, header_cpu, file_type) != (0xFEEDFACF, cpu, 6):
            raise ValueError(f"{architecture}: expected a matching 64-bit dylib")
        position = start + 32
        found_symtab = False
        found_install_name = False
        found_deployment = False
        for _ in range(command_count):
            command, length = struct.unpack_from("<II", data, position)
            if length < 8 or position + length > start + size:
                raise ValueError(f"{architecture}: invalid load command bounds")
            if command == 2:  # LC_SYMTAB offsets are relative to the slice.
                _, _, string_offset, string_size = struct.unpack_from("<IIII", data, position + 8)
                if string_offset % 8 or string_offset + string_size > size:
                    raise ValueError(
                        f"{architecture}: invalid LINKEDIT string pool at {string_offset:#x}"
                    )
                found_symtab = True
                print(f"{architecture}: LINKEDIT string pool aligned at {string_offset:#x}", flush=True)
            elif command in (0x0C, 0x0D, 0x80000018, 0x8000001F):
                name_offset = struct.unpack_from("<I", data, position + 8)[0]
                name_end = data.find(b"\0", position + name_offset, position + length)
                if name_offset < 24 or name_end < 0:
                    raise ValueError(f"{architecture}: invalid dylib name")
                name = data[position + name_offset:name_end].decode()
                if command == 0x0D:
                    if name != "@rpath/libscriptmetakit_ffi.dylib":
                        raise ValueError(f"{architecture}: unexpected install name {name}")
                    found_install_name = True
                elif not name.startswith(("/usr/lib/", "/System/Library/")):
                    raise ValueError(f"{architecture}: non-system runtime dependency {name}")
            elif command in (0x32, 0x24):
                if command == 0x32:
                    platform, minimum = struct.unpack_from("<II", data, position + 8)
                    if platform != 1:
                        raise ValueError(f"{architecture}: expected the macOS platform")
                else:
                    minimum = struct.unpack_from("<I", data, position + 8)[0]
                if minimum > (13 << 16):
                    raise ValueError(f"{architecture}: deployment target exceeds macOS 13")
                found_deployment = True
            position += length
        if not found_symtab:
            raise ValueError(f"{architecture}: missing LC_SYMTAB")
        if not found_install_name or not found_deployment:
            raise ValueError(f"{architecture}: missing install name or deployment target")
        found.add(architecture)
    if found != set(ARCHITECTURES.values()) or count != 2:
        raise ValueError("expected exactly arm64 and x86_64")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library", type=Path)
    parser.add_argument("--expected-version", default=None)
    arguments = parser.parse_args()
    library = arguments.library.resolve(strict=True)
    root = Path(__file__).resolve().parent.parent
    version = arguments.expected_version or re.search(r'^version = "([^"]+)"', (root / "Cargo.toml").read_text(), re.M).group(1)
    header = (root / "scriptmetakit_ffi/include/scriptmetakit_ffi.h").read_text()
    expected_symbols = set(re.findall(r"\b(smk_\w+)\s*\(", header))
    validate_string_pools(library)
    with tempfile.TemporaryDirectory(prefix="scriptmetakit-native-link-") as temporary:
        for architecture in ARCHITECTURES.values():
            exported = subprocess.check_output(["nm", "-arch", architecture, "-gU", str(library)], text=True)
            symbols = set(re.findall(r"\b_(smk_\w+)\b", exported))
            missing = expected_symbols - symbols
            if missing:
                raise ValueError(f"{architecture}: missing public functions: {', '.join(sorted(missing))}")
            print(f"{architecture}: all {len(expected_symbols)} public C functions exported", flush=True)
            executable = Path(temporary) / f"probe-{architecture}"
            subprocess.run(
                [
                    "/usr/bin/clang", "-arch", architecture, "-mmacosx-version-min=13.0",
                    "-I", str(root / "scriptmetakit_ffi/include"),
                    str(root / "tests/native_link_probe.c"), str(library),
                    f"-Wl,-rpath,{library.parent}", "-o", str(executable),
                ],
                check=True,
            )
            print(f"{architecture}: native link passed", flush=True)
            available = subprocess.run(
                ["/usr/bin/arch", f"-{architecture}", "/usr/bin/true"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            ).returncode == 0
            if available:
                native_arch = "aarch64" if architecture == "arm64" else architecture
                subprocess.run(["/usr/bin/arch", f"-{architecture}", str(executable), version, native_arch], check=True)
                print(f"{architecture}: runtime version, capabilities and lifecycle passed", flush=True)
            else:
                print(f"{architecture}: execution unavailable on this host; link checked", flush=True)


if __name__ == "__main__":
    main()
