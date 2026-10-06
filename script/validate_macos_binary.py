#!/usr/bin/env python3
"""Validate the Universal dylib's string pools, link both slices, and run available CPUs."""

import argparse
from pathlib import Path
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
            position += length
        if not found_symtab:
            raise ValueError(f"{architecture}: missing LC_SYMTAB")
        found.add(architecture)
    if found != set(ARCHITECTURES.values()) or count != 2:
        raise ValueError("expected exactly arm64 and x86_64")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library", type=Path)
    arguments = parser.parse_args()
    library = arguments.library.resolve(strict=True)
    root = Path(__file__).resolve().parent.parent
    validate_string_pools(library)
    with tempfile.TemporaryDirectory(prefix="scriptmetakit-native-link-") as temporary:
        for architecture in ARCHITECTURES.values():
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
                subprocess.run(["/usr/bin/arch", f"-{architecture}", str(executable)], check=True)
                print(f"{architecture}: engine create/free passed", flush=True)
            else:
                print(f"{architecture}: execution unavailable on this host; link checked", flush=True)


if __name__ == "__main__":
    main()
