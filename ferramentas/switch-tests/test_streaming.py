"""Run the real streaming modules on the WSL host without linking GLES."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

tests = Path(__file__).resolve().parent
repo = tests.parents[1]
source = tests / "horizon-client-switch/src"
vanilla = sys.argv[1:] == ["--vanilla"]
if sys.argv[1:] and not vanilla:
    raise SystemExit("Usage: test_streaming.py [--vanilla]")

with tempfile.TemporaryDirectory(prefix="horizon-switch-streaming-") as folder:
    crate = Path(folder)
    (crate / "Cargo.toml").write_text(
        '[package]\nname = "horizon-switch-streaming-tests"\nversion = "0.0.0"\nedition = "2024"\n'
        '[lib]\npath = "lib.rs"\n[workspace]\n[features]\nvanilla = []\n[dev-dependencies]\n'
        + "".join(
            f"{name} = {{ path = {json.dumps(str(repo / 'crates' / name))} }}\n"
            for name in ("horizon-core", "horizon-world")
        )
    )
    modules = {
        "block_palette": source / "block_palette.rs",
        "world_stream": source / "world_stream.rs",
        "ticket_levels": repo / "crates/horizon-client/src/ticket_levels.rs",
    }
    if vanilla:
        modules["vanilla"] = source / "vanilla.rs"
    (crate / "lib.rs").write_text(
        "#![cfg(test)]\n#![allow(dead_code)]\nuse horizon_core::fast_hash;\n"
        + "".join(f"#[path = {json.dumps(str(path))}]\nmod {name};\n" for name, path in modules.items())
    )
    subprocess.run(
        ["cargo", "test", "--offline", "--manifest-path", str(crate / "Cargo.toml"),
         *(["--features", "vanilla", "--release", "--", "--nocapture"] if vanilla else [])],
        env={**os.environ, "HORIZON_STREAM_TEST_ROOT": folder,
             "HORIZON_VANILLA_ROOT": str(tests / "out/test7-worldgen-romfs")},
        check=True,
    )
