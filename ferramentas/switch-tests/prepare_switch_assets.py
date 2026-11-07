#!/usr/bin/env python3
"""
Prepares a minimal, clean Minecraft 26.3 asset bundle for Nintendo Switch (`test4_horizon_renderer`).

- Verifies the local official Minecraft 26.3 installation (`26.3.jar` and `26.3.json`) against
  `research-lock.json` SHA-1 digests.
- Extracts ONLY the transitive closure of blockstates, block models, PNG block textures, and
  `.png.mcmeta` animation descriptors required by `test4_horizon_renderer` into:
  `ferramentas/switch-tests/out/sdcard/switch/horizon/assets/`
  (ready for `sdmc:/switch/horizon/assets/` on the Switch microSD card and `romfs:/assets/`).
- Never modifies tracked repository files or commits proprietary assets.
"""

import hashlib
import json
import os
import shutil
import sys
import zipfile
from pathlib import Path

BLOCKS = [
    "grass_block",
    "dirt",
    "stone",
    "bedrock",
    "sand",
    "oak_log",
    "oak_leaves",
    "gold_block",
    "sea_lantern",
    "water",
]

EXTRA_TEXTURES = [
    "block/water_still",
]

VANILLA_BLOCKS = [
    "deepslate",
    "gravel",
    "tuff",
    "deepslate_iron_ore",
    "iron_ore",
    "coal_ore",
    "copper_ore",
    "granite",
    "diorite",
    "andesite",
    "sandstone",
    "clay",
    "snow_block",
    "ice",
    "calcite",
    "lava",
    "red_sand",
    "terracotta",
    "coarse_dirt",
    "podzol",
    "mud",
    "moss_block",
    "deepslate_diamond_ore",
]


VISUAL_BLOCKS = ['birch_log', 'birch_leaves', 'spruce_log', 'spruce_leaves', 'dark_oak_log', 'dark_oak_leaves', 'jungle_log', 'jungle_leaves', 'acacia_log', 'acacia_leaves', 'cherry_log', 'cherry_leaves', 'mangrove_log', 'mangrove_leaves', 'oak_sapling', 'short_grass', 'fern', 'dead_bush', 'dandelion', 'poppy', 'blue_orchid', 'allium', 'azure_bluet', 'oxeye_daisy', 'cornflower', 'lily_of_the_valley', 'brown_mushroom', 'red_mushroom', 'sugar_cane', 'lily_pad', 'cactus', 'glass']

VISUAL_BLOCKS += ["brown_mushroom_block", "red_mushroom_block", "mushroom_stem"]

def sha1_file(path: Path) -> str:
    h = hashlib.sha1()
    with path.open("rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def strip_ns(ident: str) -> str:
    return ident.split(":", 1)[1] if ":" in ident else ident


def find_minecraft_26_3() -> tuple[Path, Path]:
    candidates = []
    if env_mc := os.environ.get("HORIZON_MINECRAFT_DIR"):
        candidates.append(Path(env_mc))
    candidates.extend([
        Path("/mnt/c/Users/Pc vinw/AppData/Roaming/.minecraft"),
        Path.home() / ".minecraft",
    ])
    for base in candidates:
        jar = base / "versions" / "26.3" / "26.3.jar"
        manifest = base / "versions" / "26.3" / "26.3.json"
        if jar.is_file() and manifest.is_file():
            return jar, manifest
    raise FileNotFoundError("Could not locate Minecraft 26.3 installation (versions/26.3/26.3.jar)")


def main() -> int:
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent.parent
    lock_path = repo_root / "research-lock.json"
    lock_data = json.loads(lock_path.read_text(encoding="utf-8"))["minecraft"]

    jar_path, manifest_path = find_minecraft_26_3()
    jar_sha1 = sha1_file(jar_path)
    manifest_sha1 = sha1_file(manifest_path)

    expected_jar_sha1 = lock_data["client_sha1"]
    accepted_manifests = [lock_data["manifest_sha1"], *lock_data.get("accepted_manifest_sha1", [])]

    if jar_sha1 != expected_jar_sha1:
        raise SystemExit(f"26.3.jar SHA-1 mismatch: got {jar_sha1}, expected {expected_jar_sha1}")
    if manifest_sha1 not in accepted_manifests:
        raise SystemExit(f"26.3.json SHA-1 mismatch: got {manifest_sha1}")

    visuals = sys.argv[1:] == ["--vanilla-visuals"]
    water = visuals or sys.argv[1:] == ["--vanilla-water"]
    vanilla = water or sys.argv[1:] == ["--vanilla"]
    if sys.argv[1:] and not vanilla:
        raise SystemExit("Usage: prepare_switch_assets.py [--vanilla|--vanilla-water|--vanilla-visuals]")
    blocks = BLOCKS + VANILLA_BLOCKS if vanilla else BLOCKS
    if visuals:
        blocks += VISUAL_BLOCKS
    out_root = (script_dir / "out" / ("test10-romfs" if visuals else "test8f-romfs" if water else "test8-romfs") if vanilla
                else script_dir / "out" / "sdcard" / "switch" / "horizon")
    assets_root = out_root / "assets"
    if assets_root.exists() and not vanilla:
        shutil.rmtree(assets_root)
    assets_root.mkdir(parents=True, exist_ok=True)

    extracted_files: list[str] = []
    models_queue: list[str] = []
    visited_models: set[str] = set()
    textures_set: set[str] = set(EXTRA_TEXTURES + (["block/lava_still"] if vanilla else []))

    if water:
        textures_set.update(["block/water_flow", "block/water_overlay"])

    if visuals:
        models_queue.append("item/water_bucket")
        textures_set.update(f"block/destroy_stage_{i}" for i in range(10))
        textures_set.update(["particle/bubble", *[f"particle/splash_{i}" for i in range(4)]])

    with zipfile.ZipFile(jar_path, "r") as zf:
        jar_names = set(zf.namelist())

        def extract_entry(entry_path: str) -> bytes:
            if entry_path not in jar_names:
                raise FileNotFoundError(f"Missing entry in 26.3.jar: {entry_path}")
            data = zf.read(entry_path)
            dst = out_root / entry_path
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_bytes(data)
            extracted_files.append(entry_path)
            return data

        if visuals:
            for name in ("grass", "foliage"):
                extract_entry(f"assets/minecraft/textures/colormap/{name}.png")

        # 1. Extract blockstates and collect referenced models
        for block in blocks:
            bs_entry = f"assets/minecraft/blockstates/{block}.json"
            raw = extract_entry(bs_entry)
            bs_json = json.loads(raw.decode("utf-8"))

            def collect_models_from_variant(v):
                if isinstance(v, list):
                    for item in v:
                        if isinstance(item, dict) and "model" in item:
                            models_queue.append(strip_ns(item["model"]))
                elif isinstance(v, dict) and "model" in v:
                    models_queue.append(strip_ns(v["model"]))

            if "variants" in bs_json:
                for v in bs_json["variants"].values():
                    collect_models_from_variant(v)
            if "multipart" in bs_json:
                for case in bs_json["multipart"]:
                    if "apply" in case:
                        collect_models_from_variant(case["apply"])

        # 2. Extract models and follow parent chains + texture variables
        while models_queue:
            model_id = strip_ns(models_queue.pop(0))
            if model_id in visited_models:
                continue
            visited_models.add(model_id)

            model_entry = f"assets/minecraft/models/{model_id}.json"
            raw = extract_entry(model_entry)
            model_json = json.loads(raw.decode("utf-8"))

            if parent := model_json.get("parent"):
                parent_id = strip_ns(parent)
                if not parent_id.startswith("builtin/"):
                    models_queue.append(parent_id)

            for tex_val in model_json.get("textures", {}).values():
                if isinstance(tex_val, dict):
                    tex_val = tex_val.get("sprite")
                if isinstance(tex_val, str) and not tex_val.startswith("#"):
                    textures_set.add(strip_ns(tex_val))

        # 3. Extract PNG textures and optional .png.mcmeta files
        for tex_id in sorted(textures_set):
            png_entry = f"assets/minecraft/textures/{tex_id}.png"
            extract_entry(png_entry)
            mcmeta_entry = f"{png_entry}.mcmeta"
            if mcmeta_entry in jar_names:
                extract_entry(mcmeta_entry)

    manifest_info = {
        "version": "26.3",
        "client_jar_sha1": jar_sha1,
        "manifest_json_sha1": manifest_sha1,
        "blocks": blocks,
        "models": sorted(visited_models),
        "textures": sorted(textures_set),
        "files": sorted(extracted_files),
    }
    manifest_out = assets_root / "manifest.json"
    manifest_out.write_text(json.dumps(manifest_info, indent=2) + "\n", encoding="utf-8")

    total_bytes = sum((out_root / f).stat().st_size for f in extracted_files) + manifest_out.stat().st_size
    print(
        f"Prepared Switch 26.3 asset bundle in {assets_root}: "
        f"{len(blocks)} blockstates, {len(visited_models)} models, "
        f"{len(textures_set)} textures, {len(extracted_files)} files ({total_bytes} bytes)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
