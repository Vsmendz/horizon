#!/usr/bin/env python3
"""Stage the original Horizon worldgen bundle and an independent 26.3 fixture."""

import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from test_chunk_generation_parity import Oracle, parse


def fingerprint(parts):
    value = 0xcbf29ce484222325
    for part in parts:
        for byte in part:
            value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--resources", type=Path, default=ROOT / ".local/horizon-client/26.3",
        help="Horizon 26.3 resources produced by horizon-prepare",
    )
    resources = parser.parse_args().resources.resolve()
    if not (resources / "worldgen/block_flags.json").is_file():
        raise SystemExit("Run horizon-prepare for Minecraft 26.3 first")
    seed, x, z = 12345, -2, 3
    output = HERE / "out/preparation/vanilla-terrain.txt"
    output.parent.mkdir(parents=True, exist_ok=True)
    os.environ["HORIZON_PHASE"] = "terrain"
    with tempfile.TemporaryDirectory(prefix="horizon-switch-java-") as scratch:
        os.chdir(scratch)
        try:
            Oracle().run(output, ["dump", seed, "overworld", f"{x},{z},{x},{z}"])
        finally:
            os.chdir(ROOT)
    parsed = parse(output)
    if set(parsed) != {(x, z)}:
        raise SystemExit("Expected one column from the original Java oracle")
    chunk = parsed[(x, z)]
    ys = sorted(chunk["y"])
    qys = sorted(chunk["q"])
    if not ys or ys != list(range(ys[0], ys[-1] + 1)):
        raise SystemExit("Java oracle returned a non-contiguous block height")
    if qys != list(range(ys[0] // 4, ys[-1] // 4 + 1)):
        raise SystemExit("Java oracle returned an unexpected quart height")
    blocks = [chunk["y"][y].split() for y in ys]
    biomes = [chunk["q"][y].split() for y in qys]
    if any(len(row) != 256 for row in blocks) or any(len(row) != 16 for row in biomes):
        raise SystemExit("Java oracle returned an incomplete layer")
    block_hash = fingerprint(struct.pack("<I", int(state)) for row in blocks for state in row)
    biome_hash = fingerprint(biome.encode() + b"\0" for row in biomes for biome in row)
    dimension = json.loads(
        (resources / "data/minecraft/dimension_type/overworld.json").read_text(encoding="utf-8")
    )
    if (ys[0], len(ys)) != (dimension["min_y"], dimension["height"]):
        raise SystemExit("Java column dimensions differ from the prepared overworld dimension")
    stage = HERE / "out/test7-worldgen-romfs"
    stage.mkdir(exist_ok=True)
    shutil.copy2(resources / "facts.json", stage)
    behaviour = resources / "block-behaviours.json"
    if behaviour.is_file():
        shutil.copy2(behaviour, stage)
    shutil.copytree(resources / "worldgen", stage / "worldgen", dirs_exist_ok=True)
    fixture = f"{seed} {x} {z} {ys[0]} {len(ys)} {block_hash:016x} {biome_hash:016x}\n"
    (stage / "expected.txt").write_text(fixture, encoding="ascii")
    print(f"Java terrain fixture {fixture.strip()}")
    print(f"Compared dimension height; prepared {stage}")


if __name__ == "__main__":
    main()
