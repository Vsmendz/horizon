"""Check a Switch ELF/NRO, its staged RomFS and preserved milestone hashes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def romfs_files(data):
    header = struct.unpack_from("<10Q", data)
    assert header[0] == 0x50
    _, dh, dhs, dirs, ds, fh, fhs, files, fs, contents = header
    for off, size in ((dh, dhs), (dirs, ds), (fh, fhs), (files, fs)):
        assert off + size <= len(data)
    result, visited_dirs, visited_files = {}, set(), set()

    def visit(offset, prefix):
        assert offset not in visited_dirs and offset + 24 <= ds
        visited_dirs.add(offset)
        _, sibling, child, file, _, length = struct.unpack_from("<6I", data, dirs + offset)
        assert offset + 24 + length <= ds
        name = data[dirs + offset + 24:dirs + offset + 24 + length].decode()
        path = prefix + name + ("/" if name else "")
        while file != 0xFFFFFFFF:
            assert file not in visited_files and file + 32 <= fs
            visited_files.add(file)
            _, nxt, start, size, _, length = struct.unpack_from("<IIQQII", data, files + file)
            assert file + 32 + length <= fs and contents + start + size <= len(data)
            name = data[files + file + 32:files + file + 32 + length].decode()
            key = path + name
            assert key not in result
            result[key] = data[contents + start:contents + start + size]
            file = nxt
        if child != 0xFFFFFFFF:
            visit(child, path)
        if sibling != 0xFFFFFFFF:
            visit(sibling, prefix)

    visit(0, "")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name")
    parser.add_argument("stage", type=Path)
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--history-root", type=Path, help="checkout containing the earlier NROs")
    parser.add_argument("--allow-missing-history", action="store_true")
    args = parser.parse_args()
    tests = Path(__file__).resolve().parent
    repo = args.history_root or tests.parents[1]
    out = args.artifacts or tests / "out"
    nro, elf = (out / (args.name + ext) for ext in (".nro", ".elf"))
    data = nro.read_bytes()
    assert data[0x10:0x14] == b"NRO0"
    end = struct.unpack_from("<I", data, 0x18)[0]
    assert 0x80 <= end <= len(data)
    for pos in (0x20, 0x28, 0x30):
        start, size = struct.unpack_from("<II", data, pos)
        assert start + size <= end
    assert data[end:end + 4] == b"ASET"
    ranges = struct.unpack_from("<6Q", data, end + 8)
    for start, size in zip(ranges[::2], ranges[1::2]):
        assert end + start + size <= len(data)
    romoff, romsize = ranges[4:]
    packaged = romfs_files(data[end + romoff:end + romoff + romsize])
    staged = {p.relative_to(args.stage).as_posix(): p.read_bytes()
              for p in args.stage.rglob("*") if p.is_file()}
    assert packaged.keys() == staged.keys(), "RomFS entry mismatch"
    for name, contents in staged.items():
        assert packaged[name] == contents, name
    prefix = Path(os.environ["DEVKITA64"]) / "bin/aarch64-none-elf-"

    def run(tool, *opts):
        return subprocess.check_output([str(prefix) + tool, *opts, str(elf)], text=True)

    header = run("readelf", "-h")
    assert "ELF64" in header and "AArch64" in header
    assert not run("nm", "-u").strip(), "undefined ELF symbols"
    assert "(NEEDED)" not in run("readelf", "-d"), "dynamic dependencies"
    index = repo / "ferramentas/switch-tests/results.json"
    if args.allow_missing_history and not index.is_file():
        index = tests / "results.json"
    prior = json.loads(index.read_text())["results"]
    missing = []
    for item in prior:
        path = repo / item["file"]
        if not path.is_file() and args.allow_missing_history:
            missing.append(item["test"])
            continue
        assert digest(path) == item["sha256"], item["test"]
    print(f"ELF64 AArch64: zero undefined symbols, no dynamic dependencies")
    print(f"NRO0/ASET bounds valid; all {len(staged)} RomFS files match staging")
    print(f"Matched {len(prior) - len(missing)} of {len(prior)} recorded NRO hashes")
    if missing:
        print(f"Historical NROs unavailable for verification: {', '.join(missing)}")
    print(f"NRO {nro.name}: bytes={len(data)} sha256={digest(nro)}")
    print(f"ELF sha256={digest(elf)}")
    print("Hardware validation pending; these are offline integrity checks")


if __name__ == "__main__":
    main()
