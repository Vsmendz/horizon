# Switch port verification report — 2026-11-05

All paths below are relative to the Horizon repository unless absolute.
Hardware confirmations were supplied by the user in this chat after each nxlink upload.
No screenshot, captured framebuffer, or runtime network trace was obtained.
See results.json for artifact sizes, SHA-256 digests and individual confirmations.

## Test 10B woodland state models and platform storage isolation

The Test 10 hardware run (`test10_vanilla_world_visuals.log`) confirmed decorated streaming, 5,640 frames (58.57 FPS across 96.296 s), 46 intervals above 33 ms, a maximum interval of 108.660 ms, a first frame at 55.033 s (down from 63.860 s in Test 9 after skipping structure tables), 2,011 emitted particles (peak 138), 23 block breaks, 10 placements, two saved columns, `RESULT=0` and `RETURN 0`. The owner reported a visual defect on some woodland trees near spawn.

In the Dark Forest around spawn (`FEATURE_STATES` shows 455 `dark_oak_log`, 1,246 `dark_oak_leaves`, 45 `brown_mushroom_block`, 148 `red_mushroom_block` and 25 `mushroom_stem`), giant mushroom blocks were absent from `VISUAL_BLOCKS` and fell back to `VoxelBlock::Stone` because their collision shape is a full cube. In addition, log axes (`axis=x`/`z`) and multipart mushroom faces (`up/down/north/south/east/west` selecting outer cap versus `mushroom_block_inside` pores) previously used a single variant per `VoxelBlock` rather than per `BlockStateId`.

`test10b_woodland_models` adds `brown_mushroom_block`, `red_mushroom_block` and `mushroom_stem` to `prepare_switch_assets.py` and `VoxelBlock`, and introduces `state_models.rs` to bind `BlockStateDefinition` and `CompiledChoices` across all registered states of `*_log`, `brown_mushroom_block`, `red_mushroom_block` and `mushroom_stem` (192+ state models). The RomFS bundle now packages 68 blockstates, 100 models and 104 textures (105 atlas layers including white, 1,843 RomFS entries). `vanilla.rs` also logs `MODEL_FALLBACK stone_names=...` at startup so any remaining full-block fallback states are visible in the SD log.

On the full-client path:
- Commit `2432d280a` isolated desktop `arboard` in `horizon-client-input` (`cfg(not(target_os = "horizon"))`) and direct `rustix` calls in `horizon-client`, `horizon-client-lib` and `horizon-client-render` (`cfg(all(unix, not(target_os = "horizon")))`).
- `crates/horizon-platform` replaces `fs4` in `horizon-client` / `horizon-client-lib` (`save.rs` `estimate_disk_space`) with a `libc::statvfs` implementation on `target_os = "horizon"` (`fs4::available_space` elsewhere).
- `runtime-audit/measure_abi.py` measured `fsblkcnt_t` (8 B), `fsfilcnt_t` (4 B) and `struct statvfs` (80 B, align 8, 11 field offsets) from devkitA64 C headers, expanding `abi.json` to 371 measurements and `abi_assertions.rs` from 163 to 180 compile-time SDK checks. `test10b_woodland_models` calls `horizon_platform::available_space(Path::new("sdmc:/"))` at startup and logs `PLATFORM_STORAGE available_bytes=...`.
- In the isolated full-client check (`out/full-client-switch.remaining-deps.log`), `winit 0.30.13` is now the only remaining dependency pulling `rustix 1.1.5` into `horizon-client`.

| Gate | Result |
| --- | --- |
| AArch64 Rust, static link and ELF | PASS OFFLINE. ELF64 AArch64, zero undefined symbols, no dynamic dependencies, 180 compile-time ABI assertions |
| NRO and RomFS | PASS OFFLINE. Valid NRO0 and ASET ranges. All 1,843 RomFS entries matched staging and all 21 prior NRO digests matched (`out/test10b_woodland_models.offline.log`) |
| Woodland state models & GLES host suite | PASS OFFLINE. 35 passed, two opt-in probes skipped (`out/test10b.gles.log`, `out/test10b.models.log`). Verified log axis end faces, mushroom cap vs inside pore faces, leaf sprites and 192+ state models |
| Legacy streaming & `horizon-platform` | PASS OFFLINE. 14 passed, one ignored (`out/test10b.legacy.log`); 2 passed in `horizon-platform` (`out/platform.host.log`, `out/platform.clippy.log`) |
| Physical hardware | EXECUTED ON HARDWARE (`test10b_woodland_models.log`, `out/test10b_woodland_models.nro`, 58,713,241 bytes, SHA-256 `237eb76dd56fbadba03858918702708e63db4bf1bde1955f2b8c277858604bb1`): 4,574 frames (~59.48 FPS), 7 intervals over 33 ms, max interval 38.995 ms, first frame 62.666 s, 180 SDK ABI checks, `PLATFORM_STORAGE available_bytes=26329186304`, `FEATURES unsupported=0 failed=0`, `RESULT=0`, `RETURN 0`. Visual appearance of woodland models and hardware particle emission (`EFFECTS emitted=0`) remain pending; `MODEL_FALLBACK` listed remaining full-block `Stone` fallbacks |

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test10b_woodland_models
python3 ferramentas/switch-tests/verify_artifact.py test10b_woodland_models ferramentas/switch-tests/out/test10-romfs
./ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test10b_woodland_models.nro -s
```

## Test 10 world decoration and visual effects

The owner approved Test 9 on physical hardware. Its SD log records 33,252 frames, 59.66 FPS, 49 intervals over 33 ms, a maximum interval of 203.084 ms and a first frame at 63.860 s. It also records 19 breaks, 113 placements, one saved column, `RESULT=0` and `RETURN 0`. The previous held-water cube was oversized.

Test 10 uses the original generator's biomes, terrain and carvers and calls `GeneratorWorker::decorate` on the streaming worker. Three-by-three published tiles have a one-chunk decoration halo and a two-chunk terrain halo. The schedule is independent of request order. Conflicting writes near tile boundaries may resolve differently from Java. The runtime skips structure tables for this world. The separate save preserves registry states, quart biomes and heightmaps through the existing RegionFile and NBT storage.

The atlas has 101 layers including the white HUD layer. The HAL explicitly allocates a 2D array. Leaves and plants use alpha cutoff. Water, ice and glass use the section-sorted translucent pass. The asset exporter recognizes object-valued sprites such as glass. The water bucket uses the original item baker and first-person transform. Placement still sets a water source. Grass and leaves use biome climate colormaps and overrides. Biome sky color interpolates as the player moves. Emission improves emissive faces. Full light propagation, biome color blending, clouds and a persistent world clock remain open.

The effect pass shares the atlas, blends with depth testing and leaves the depth buffer unchanged. The 20 Hz simulation caps particle storage at 256. Block impact and destruction reuse the desktop emission geometry. Water entry and exit trigger splashes, with bubbles on entry. Holding ZR advances mining using exported Java hardness and tool requirements. D-pad left retains instant edits. Crack stages clear on release, target change or destruction.

| Gate | Result |
| --- | --- |
| AArch64 Rust, static link and ELF | PASS OFFLINE. ELF64 AArch64, zero undefined symbols and no dynamic dependencies |
| NRO and RomFS | PASS OFFLINE. Valid NRO0 and ASET ranges. All 1,831 entries matched staging and all 20 prior NRO digests matched |
| Mesa GLES and vanilla host tests | PASS OFFLINE. 34 passed, two opt-in probes skipped in the full suite. Explicit array targets at 6, 15, 36, 42, 48 and 100 layers. Covered framebuffer readback, terrain, water, HUD, particle blending and held-item shader |
| Legacy streaming | PASS OFFLINE. 14 passed, one ignored |
| Decoration | PASS OFFLINE for Rust determinism, changed terrain and NBT reconstruction. Two tiles generated 70 pristine columns, placed 3,220 features, reported zero unsupported/failed invocations and peaked at 5,028,622 counted terrain bytes. The sampled columns gained 455 dark oak logs, 1,246 dark oak leaves and 54 short grass blocks |
| Java 26.3 parity | Existing terrain fixture passed. Java parity of the Switch FEATURES schedule remains open |
| Physical hardware | AWAITING HARDWARE VALIDATION. Startup time, frame pacing and memory remain unmeasured for Test 10 |

Test 9 spent 58.470 s in the block-table task and 13.650 s in the concurrent structure-resource task. Test 10 skips the latter. These tasks overlap, so that difference does not directly translate into first-frame savings. The decorated worker also adds world generation work. Compare `INIT_TABLE`, `INIT first_frame_ms`, `FEATURES`, `EFFECTS`, `UPLOAD` and `FRAME_TIMING` on the console. The Test 9 peak interval lacks a trace tying it to a specific operation.

The opt-in host block-table probe also passed. It measured 55/372 ms for collision read/decode, 70/655 ms for properties, and 44/339 ms for support, plus 82 ms to build 6,093 collision shapes. These Linux WSL measurements separate reading from decode and validation. On this host the larger cost is decoding, so the next optimization should profile the decoder on Switch before changing the on-disk format.

### Reproduction

The build requires the Test 7 worldgen bundle and local Java 26.3 `gameplay-facts.json`. If the prepared resources lack that file, export it with a JDK 25 and the local 26.3 oracle libraries in WSL.

```sh
PYTHONPATH="$PWD/tools" python3 - <<'PYTHON'
from pathlib import Path
import oracle_harness as harness
from test_rust_parity import verify_artifacts
from test_model_baking_parity import client_libraries

lock, jars, _ = verify_artifacts()
build = Path.cwd() / "ferramentas/switch-tests/out/preparation/visual-facts"
build.mkdir(parents=True, exist_ok=True)
name = "VanillaGameplayFactsExporter"
classes, deps, _ = harness.compile_oracle(
    build, name, jars, client_libraries(), lock["minecraft"]["java_major"]
)
harness.java_run(build, classes, deps, name, [build / "gameplay-facts.json"], "export")
PYTHON

source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test10_vanilla_world_visuals
python3 ferramentas/switch-tests/test_streaming.py
HORIZON_STREAM_TEST_ROOT="$(mktemp -d)" EGL_PLATFORM=surfaceless LIBGL_ALWAYS_SOFTWARE=1   HORIZON_VANILLA_ROOT="$PWD/ferramentas/switch-tests/out/test7-worldgen-romfs"   CARGO_NET_OFFLINE=true cargo test   --manifest-path ferramentas/switch-tests/horizon-client-switch/Cargo.toml   --features render-diagnostics,vanilla-visuals --lib -- --nocapture
```

The NRO has 58,630,065 bytes and SHA-256 `83ba0e8e2e920fac39ab71b0f388b756be74423dd7615c9067d9cbecce810643`. Start Sphaira NetLoader and send it from WSL.

```sh
./ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test10_vanilla_world_visuals.nro -s
```

Check trees and leaves across chunk boundaries, the HUD, water and the held bucket. Place a water source and check transparency. Hold ZR to see impact and crack stages. Release ZR or change targets to clear the cracks. Break and place a block, save, close with + and reopen to check persistence. Inspect `sdmc:/switch/horizon-tests/test10_vanilla_world_visuals.log`.

### Full-client continuation

Winit 0.30.13 has no libnx event-loop backend. The original singleplayer entry uses a server-side `Game` and an in-memory protocol pipe, with desktop UI and audio resources required at initialization. A scratch checkout of the Test 10 commit isolated desktop dependency experiments. In that checkout, resolve the verified libc 0.2.191 patch with `cargo --config <switch-config> update -p libc@0.2.190 --precise 0.2.191`. For the check only, enable the getrandom 0.2 `custom` feature in the package being checked and set `RUSTFLAGS='--cfg getrandom_backend="custom"'` for getrandom 0.3/0.4. This selects interfaces and still requires the libnx-backed symbols at link time.

With these adjustments, standalone `cargo check -p horizon-protocol` passed for the Switch target. `cargo check -p horizon-client --no-default-features` failed in rustix 1.1.5 with 19 errors including incompatible clock IDs, poll fields, sockaddr layouts and POSIX constants. The dependency tree includes rustix directly and through fs4, winit and arboard. Keep the measured libnx/newlib ABI and replace or isolate the desktop callers. The scratch harness at `/home/c_vinwhi/.cache/horizon-full-client-check.AYuuZ1/pipe-link` passed one Linux test that exchanges a protocol 777 handshake and a reply through `Pipe::pair` on two threads. The same harness linked as ELF64 AArch64 with zero undefined symbols. It used the existing C runtime with the libnx `randomGet` provider and exercised the getrandom 0.2 and 0.3 interfaces in its entry point. The ignored logs are `out/protocol-switch.pipe-host.log` and `out/protocol-switch.elf.log`. Switch execution is pending. The next integration is a server entry independent of desktop window resources. These scratch changes were kept outside the release branch.

## SDK and desktop isolation

The suggested old path /mnt/c/Users/Pc vinw/Documents/mine/horizon/ferramentas/devkitpro did not exist.
The official package endpoint returned HTTP 403, including with dkp-pacman/6.0.2.
The SDK was downloaded from the official devkitpro/devkita64 Docker Registry image, manifest:
sha256:82575ea78651b530b2e232bb3799cfd1fe331514e053d5f724bb4b28191fb79d.
Downloaded layer SHA-256 checksums were verified before extraction.
Extraction used /tmp; the SDK was then copied into ferramentas/devkitpro (persistent, not a /tmp symlink).
ferramentas/sdk-image.json records the manifest. bootstrap_sdk.py records the retrieval procedure.
The compiler reports devkitA64 GCC 15.2.0. Rust reports 1.101.0-nightly (708c94ef0 2026-11-03).
Verified tools: aarch64-none-elf-g++, elf2nro, nacptool, nxlink.
Verified libraries: libnx.a, libEGL.a, libglad.a, libglapi.a, libdrm_nouveau.a, libSDL2.a,
newlib libc.a and libpthread.a. No guessed -lpthread was added to the graphics tests.

Source ferramentas/env.sh to set DEVKITPRO, DEVKITA64, LIBNX, PORTLIBS_PATH, PORTLIBS,
and CARGO_TARGET_DIR=/tmp/horizon-switch-target.
build.py creates /tmp/horizon-devkitpro pointing at the installed SDK to avoid spaces in
paths expanded internally by GCC specs. The SDK itself remains inside ferramentas.
The only tracked change is the /ferramentas/ ignore rule. No Horizon game sources, manifests,
desktop dependencies, global Rust sources, or installed libc crate sources were patched.
All probes and SDK files are local and ignored. No Minecraft resources were downloaded.

## Build commands (inside WSL)

From the repository root:

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/build.py test1_triangle
    python3 ferramentas/switch-tests/build.py test2a_rust
    python3 ferramentas/switch-tests/build.py test2b_rust_egl
    python3 ferramentas/switch-tests/build.py test2c_posix_c

Windows invocation: wsl bash -c 'source ferramentas/env.sh; python3 ferramentas/switch-tests/build.py test1_triangle'

Each out/<test>.build.log contains compiler/linker/packaging commands and output.
Native intermediate files use /tmp/horizon-switch-target/native.
Future build.py runs also copy ELF and map files into out/ for inspection.
The baseline came from the user-requested official switchbrew example, pinned at
669786898205b7beb25ff1731e72982e6d0397d3:
https://github.com/switchbrew/switch-examples/blob/669786898205b7beb25ff1731e72982e6d0397d3/graphics/opengl/simple_triangle/source/main.cpp
test1_origin.txt records provenance. The baseline source was used unchanged, with ENABLE_NXLINK unset.
It remains local/ignored; third-party licensing needs review before any redistribution or PR.

## Deploy commands

User confirmed Sphaira's address as 192.168.1.5. WSL routes it through its Windows NAT gateway.

    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test1_triangle.nro -s
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test2a_rust.nro -s
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test2b_rust_egl.nro -s
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test2c_posix_c.nro -s

Actual invocations wrapped nxlink in timeout 45 and stdbuf, piping output to each
out/<test>.deploy.log. nxlink reached "starting server / server active" on all four uploads.
The probes do not connect stdout to nxlink, so these files contain transport logs only.
Timeout/server shutdown exit codes are not application test results; the hardware results below
come from the user's explicit visual and exit confirmations.

## Test 1 — native OpenGL

Source: test1_triangle.cpp
Binary: out/test1_triangle.nro
Build: passed. Link: passed. NRO header: verified.
Hardware: user confirmed colored triangle on green-blue background; PLUS returned to Sphaira.
Runtime logs: no network logging enabled in baseline. Transport: out/test1_triangle.deploy.log.
Pending: none for this baseline. Docked/handheld switching and long-duration testing were not evaluated.

## Test 2A — minimal Rust

Sources: rust/src/lib.rs, rust/Cargo.toml, rust_entry.c
Binary: out/test2a_rust.nro
Build: passed without warnings after correcting allocator binding. Link: passed.
Target: aarch64-nintendo-switch-freestanding; core+alloc built with nightly build-std; no Linux std.
The allocator calls the SDK's real memalign/free. posix_memalign was not exported by this SDK.
The probe allocates a Vec of 1024 u32 values, checks sum 523776, drops it and returns through libnx.
Hardware: user confirmed the PASS message and PLUS exit.
SD log: sdmc:/switch/horizon-tests/test2a.log (not fetched back to PC).
Transport: out/test2a_rust.deploy.log.
Pending: none for this minimal test; this does not validate std or general allocator stress cases.

## Test 2B — Rust EGL/GLES

Sources: rust/src/lib.rs with feature egl, rust_entry.c
Binary: out/test2b_rust_egl.nro
Build: passed. Link: passed.
Rust calls static EGL functions over nwindowGetDefault(), requests OpenGL ES 3,
resolves GL entry points via eglGetProcAddress, clears cyan and swaps buffers.
Network logs are not required to initialize graphics.
Hardware: user confirmed cyan screen and PLUS exit.
SD log: sdmc:/switch/horizon-tests/test2b.log (not fetched back to PC).
Transport: out/test2b_rust_egl.deploy.log.
Pending: wgpu Instance/Device and shader translation are not tested by this probe.

## Test 2C — C pthread/newlib

Source: test2c_posix_c.c
Binary: out/test2c_posix_c.nro
Build: passed. Link: passed against SDK libpthread/newlib/libnx.
Checks: pthread creation/join, mutex/condition rendezvous, compiler TLS isolation,
pthread-key TLS isolation/destructor, and binary write/read/compare/remove on SD.
Hardware: user confirmed both PASS lines, RESULT=0 and PLUS exit.
SD log: sdmc:/switch/horizon-tests/test2c_c.log (not fetched back to PC).
Transport: out/test2c_posix_c.deploy.log.
Pending: none for the C probe. This is NOT evidence that Rust std works.

## Test 2C — Rust std/newlib audit (PASSED ON HARDWARE)

Sources: runtime-audit/probe.rs, runtime-audit/abi_assertions.rs, runtime-audit/Cargo.toml,
runtime-audit/cargo-switch.toml, runtime-audit/aarch64-nintendo-switch-newlib.json,
std_entry.c, and vendor/libc (patched via runtime-audit/patch_libc.py).
Binary: out/test2c_rust_std.nro (379,077 bytes, SHA-256 1b676f7751c537ec3038cc8cc80d378069dd34e2e6c19c9a68558c37d298e750).
ELF: out/test2c_rust_std.elf (4,020,840 bytes). Map: out/test2c_rust_std.map.

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/runtime-audit/measure_abi.py
    python3 ferramentas/switch-tests/runtime-audit/patch_libc.py
    python3 ferramentas/switch-tests/runtime-audit/generate_assertions.py
    python3 ferramentas/switch-tests/build.py test2c_rust_std

Build: passed (zero errors, zero warnings; 163 compile-time ABI layout/offset/initializer assertions).
Link: passed (zero undefined symbols; RELR dynamic relocations supported by libnx __nx_dynamic).
NRO: produced and verified (NRO0 magic at 0x10, header size 356,352 + NACP/icon).
Hardware: user_confirmed_pass on Nintendo Switch (192.168.1.5). User confirmed exact output:
- `BEGIN Rust std: ABI assertions passed at compile time`
- `PASS Rust std: thread, Mutex, Condvar, TLS and join`
- `PASS Rust std: file write/read/remove`
- `RESULT=0; press PLUS to exit`
followed by clean exit via PLUS button without freeze, error, or console crash.

Root causes resolved:
1. 19 libc 0.2.191 errors on AArch64 newlib: measured 334 ABI layout/size/align/offset/constant facts
   and exact initializers directly with devkitA64 aarch64-none-elf-gcc (abi.json, abi_initializers.json)
   and added an isolated Switch module (vendor/libc/src/unix/newlib/switch.rs) leaving 3DS
   horizon/mod.rs untouched.
2. 5 std type errors on pollfd.events/revents: updated POLL* constants in switch.rs from c_int to c_short.
3. Missing sysconf symbol at link time: devkitARM newlib includes libc_a-sysconf.o (returning 0x1000 for
   _SC_PAGESIZE=8 and -1/EINVAL otherwise), whereas devkitA64 newlib omits sysconf.o. Implemented
   horizon_switch_sysconf / sysconf in std_entry.c matching Horizon OS 4 KiB page granularity (0x1000).
4. Missing getrandom symbol for std::sys::random / HashMap: implemented getrandom in std_entry.c backed
   by libnx's OS-seeded ChaCha20 CSPRNG (randomGet in switch/kernel/random.h).

## Test 3 — Horizon Integration (PASSED ON HARDWARE)

Sources: horizon_entry.c, horizon-integration/Cargo.toml, horizon-integration/cargo-switch.toml,
horizon-integration/src/lib.rs, and real repository crates:
- crates/horizon-core (VANILLA_VERSION 26.3, PROTOCOL_VERSION 777, DATA_VERSION 5023, LegacyRandom, XoroshiroRandom, PerlinNoise, BlockPos, ChunkPos, Direction)
- crates/horizon-world (PalettedContainer, BlockState, NBT serialization/deserialization, level::ticks::Ticks)
- crates/horizon-client-core (GameRules, Scoreboard, Identifier, Text)
- crates/horizon-items (ItemStack, Cooldowns)
- third_party/naga (WGSL parser, validator, and GLSL ES 300 backend)
- glow 0.17.0, glam 0.30.10, bytemuck 1.25.0

Binary: out/test3_horizon_integration.nro (7,260,357 bytes, SHA-256 9a438f2fa8afbf2deb9682f4ee217115d79e241ab3555494c0310600199f5204).
ELF: out/test3_horizon_integration.elf (12,750,144 bytes). Map: out/test3_horizon_integration.map.

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/build.py test3_horizon_integration

Build: passed (zero errors, zero warnings; 163 compile-time ABI assertions).
Link: passed (zero undefined symbols, zero Linux svc #0 instructions).
NRO: produced and verified (NRO0 magic at 0x10, header size 7,237,632 + NACP/icon).
SD log: sdmc:/switch/horizon-tests/test3_horizon_integration.log (copied to out/test3_horizon_integration.hardware.log).
Hardware: user_confirmed_pass on Nintendo Switch (192.168.1.5).
User confirmed 3D voxel island rendering (terrain, oak tree, pulsing beacon), controller input response, camera movement, and clean PLUS exit.
Hardware log verified:
- `PASS Horizon crates: version=26.3 proto=777 data=5023 palette=10 vertices=6312`
- `PASS Naga WGSL->GLSL ES 300 translation (vs=1514 bytes, fs=850 bytes)`
- `PASS EGL/GLES3 context active: renderer='NV120' version='OpenGL ES 3.2 Mesa 20.1.0-rc3'`
- `PASS GPU program compiled and linked from Naga GLSL ES 300 output`
- `PASS Integrated 'Server thread' spawned (20 TPS loop running)`
- `PASS Clean shutdown: frames_rendered=284 server_ticks=95 mesh_generations=10 palette_entries=10`
- `RESULT=0` / `RETURN 0`

What this test validates on hardware:
1. Full Horizon core/world/client-core/items subsystem initialization on Switch (PalettedContainer 16x16x16 voxel island, NBT round-trip, LegacyRandom + PerlinNoise terrain generation, Ticks, GameRules, Cooldowns, SD card file round-trip).
2. Runtime WGSL -> GLSL ES 300 shader translation using Horizon's third_party/naga and GPU compilation/linking via glow over EGL/GLES3 on nwindowGetDefault() with 24-bit depth buffer.
3. Dual-thread Horizon architecture: background "Server thread" ticking at 20 TPS (50 ms) pulsing the central beacon and regenerating the chunk on demand, synchronized via Arc<Mutex<SharedWorldState>> with the 60 FPS render/input thread.
4. Interactive Switch controller input: Left/Right sticks and D-Pad orbit camera, L/R zoom, A cycles sky atmosphere (Overworld Day, Sunset, Night, End), B pauses/resumes orbit, X triggers new seed terrain generation on the Server thread, Y resets camera, and PLUS cleanly joins the Server thread, releases GL/EGL resources, logs PASS + RESULT=0, and returns to Sphaira.

## Test 4 — Horizon Real Renderer: wgpu-hal GLES + horizon-assets (PASSED ON HARDWARE)

Sources: renderer_entry.c, prepare_switch_assets.py, horizon-renderer/Cargo.toml,
horizon-renderer/cargo-switch.toml, horizon-renderer/src/lib.rs, vendor/wgpu-hal (adapted wgpu-hal v30.0.1
with src/gles/horizon_egl.rs), and real repository crates:
- crates/horizon-assets (blockstate::parse_blockstate_json, resolve::ModelLibrary, bake::bake_cuboid_quads, atlas::pack_atlas_rgba8)
- crates/horizon-core, crates/horizon-world, crates/horizon-client-core, crates/horizon-items
- third_party/naga (WGSL parser, validator, and GLSL ES backend invoked by wgpu-hal)
- png 0.18, glam 0.30.10, bytemuck 1.25.0, wgpu-types 30.0.0

Binary: out/test4_horizon_renderer.nro (7,858,809 bytes, SHA-256 373792bffbbe82e4f8aa12a34380d3222fcfb99214b2022b1c9cadd0850bd0cf).
ELF: out/test4_horizon_renderer.elf (13,601,304 bytes, SHA-256 8dc1ac840e8c13c0e12a9e0f8596afff0d6c3167c5d15ce2ff1523c10bebce63).
Map: out/test4_horizon_renderer.map.

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/build.py test4_horizon_renderer

Deploy command:

    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test4_horizon_renderer.nro -s

Build: passed (zero errors; 163 compile-time ABI assertions).
Link: passed (0 undefined symbols, 0 Linux svc #0 instructions, 29 Horizon OS libnx svc instructions).
NRO: produced and verified (NRO0 magic at 0x10, embedded RomFS with 50 verified 26.3 asset files + manifest.json).
SD log: sdmc:/switch/horizon-tests/test4_horizon_renderer.log.
SD assets: sdmc:/switch/horizon/assets/ (automatically populated from romfs:/ on startup if missing on SD card).
Hardware: user_confirmed_pass on Nintendo Switch (192.168.1.5).
User confirmed:
- Correct rendering of the 3D voxel island with original Minecraft 26.3 textures.
- Central block alternating between gold_block and sea_lantern.
- Controller input working, blocks visually broken and placed.
- Clean exit via PLUS button without freeze or crash.
Hardware log verified:
- `PASS Horizon 26.3 assets loaded and baked via horizon-assets`
- `PASS Horizon chunk meshed from baked ModelQuads`
- `PASS wgpu-hal Adapter exposed`
- `PASS wgpu-hal Device, Queue, and Surface configured`
- `PASS wgpu-hal RenderPipeline`
- `PASS First 3D frame rendered and presented`
- `PASS Clean shutdown: frames_rendered=2000 server_ticks=667 mesh_generations=52`
- `RESULT=0` / `RETURN 0`

What this test validates on hardware:
1. Real `wgpu-hal` v30.0.1 GLES backend (`vendor/wgpu-hal/src/gles/horizon_egl.rs`) running on Nintendo Switch (`target_os = "horizon"`) over static `libnx` EGL 1.5 + OpenGL ES 3.2 (`NV120`, `Mesa 20.1.0-rc3`) without `dlopen`/`libloading`/`khronos-egl`/`wayland-sys`:
   - `wgpu_hal::gles::Adapter::new_external` -> `OpenDevice { device, queue }`
   - `wgpu_hal::gles::Surface::new_external` -> `configure`, `acquire_texture`, `present` (offscreen swapchain renderbuffer + Y-flip `glBlitFramebuffer` to default framebuffer `0`, matching `wgpu-hal`'s `WriterFlags::ADJUST_COORDINATE_SPACE`), `unconfigure`
   - `Device::create_shader_module` (Naga WGSL -> GLSL ES 300 via `wgpu-hal`'s internal `ShaderCache`), `create_bind_group_layout`, `create_pipeline_layout`, `create_render_pipeline`, `create_buffer` (Vertex, Uniform, CopySrc/CopyDst staging), `create_texture` (`Rgba8Unorm` atlas + `Depth24Plus` depth buffer), `create_texture_view`, `create_sampler`, `create_bind_group`, `create_command_encoder`, `create_fence`
   - `CommandEncoder::begin_coding`, `transition_textures`, `copy_buffer_to_texture`, `begin_render_pass`, `set_render_pipeline`, `set_bind_group`, `set_vertex_buffer`, `draw`, `end_render_pass`, `end_encoding`
   - `Queue::submit` and `Surface::present`
2. Real `horizon-assets` pipeline on Switch:
   - Extracts and syncs 50 real Minecraft 26.3 asset files (10 blockstates, 23 block models, 14 PNG textures, 3 `.png.mcmeta`, `21,135` bytes from the user's verified local `.minecraft/versions/26.3/26.3.jar`) to `sdmc:/switch/horizon/assets/`.
   - Parses blockstates with `parse_blockstate_json`, resolves block parent chains (`cube_all`, `cube_column`, `cube_bottom_top`, `grass_block` multi-element overlay, `leaves`) with `ModelLibrary::resolve_block_model`, decodes PNG textures with `png::Decoder`, packs the `Rgba8Unorm` texture atlas (`64x64`, 12 sprites) with `pack_atlas_rgba8`, and bakes `ModelQuad`s with `bake_cuboid_quads` (including multi-layer `grass_block_side_overlay` tinting with `CompareFunction::LessEqual` + alpha cutout).
3. Interactive 3D voxel island + 20 TPS integrated `Server thread` + controller input (`Sticks`/`D-Pad` camera, `L`/`R` zoom, `A` atmosphere cycle, `B` pause/resume orbit, `X` new seed regeneration, `Y` reset camera, `+` clean exit).

## Test 5 — First-person Horizon prototype (tested on hardware)

The recovered crate already contained the four-column world, Switch input bridge, player state, saving, and the GLES renderer. The WSL build reproduced 41 Rust errors. `horizon-client-switch/src/lib.rs` now uses the current camera entity and alignment API, delegates detached camera collision to Horizon's camera and raycast code, uses Horizon's FOV and view bob, and calls the owned-shape variant of the step resolver. Upload, presentation, thread join, and shutdown save errors now produce a nonzero result.

The prototype exercises the original Horizon camera, travel, jumping, collision, raycast, inventory, heightmap, palette, region, and asset code. Its Switch loop uses the Test 4 wgpu-hal GLES adapter over libnx EGL. The map is a fixed 2×2 set of chunk columns, 32 blocks high, with a prototype hotbar and a dedicated SD save. Desktop chunk streaming, screens, and the full client loop remain future work.

    source ferramentas/env.sh
    CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test5_horizon_client
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test5_horizon_client.nro -s

Build and link passed with the existing Rust nightly, custom newlib target, and devkitA64. Cargo emits an unused Naga patch warning in a dependency graph. The Test 5 lockfile resolves Naga to the local copy. The ELF is AArch64 ELF64 with no undefined symbols or dynamic dependencies and 29 libnx SVC instructions with no SVC 0. The link map includes the Switch client, EGL, glapi, Nouveau, libnx, libsysbase, and newlib. The NRO header and icon, NACP, and RomFS bounds passed inspection. The packaged assets come from the existing local 26.3 preparation step. See `out/test5_horizon_client.build.log` and `out/test5_horizon_client.offline.log`.

- NRO `out/test5_horizon_client.nro` is 8,141,433 bytes with SHA-256 `fee9b5864cc65f3e21b5c65d6c3afa2b5d015b0000ee50634e1eaf29aa3ed2eb`.
- ELF `out/test5_horizon_client.elf` is 14,000,640 bytes with SHA-256 `6e2162de11eb3790b4129bebc3969fcb15b46463bc85984c7f9ca54c90b6a273`.
- Runtime log is `sdmc:/switch/horizon-tests/test5_horizon_client.log`. The prototype save is in `sdmc:/switch/horizon/saves/switch_world/`.
- Expect textured terrain, a first-person camera, hotbar, held block, and a target outline. Confirm that gravity settles the player, terrain blocks movement, and a jump returns to the ground.
- Left stick moves, right stick looks, L3 toggles sprint, R3 toggles crouch, B jumps, ZR or D-pad left breaks, A or ZL places, L/R selects hotbar slots, D-pad up cycles perspective, X saves, Y respawns, and PLUS exits.
- Edit blocks, save, exit with PLUS, then reopen to check persistence. Look for a presented first frame, positive frame and tick counts, four saved columns, `RESULT=0`, and `RETURN 0`. If nxlink is unavailable, copy the NRO to the SD card's `switch` directory and launch it from Sphaira.

The user confirmed movement, horizontal camera, jump and gravity, block collision, break and place, hotbar selection, save and reload persistence, and clean PLUS exit on hardware. Vertical look was inverted, the held block appeared distorted, and the SD log was absent.

## Test 5B — Hardware follow-up (confirmed)

- **Pitch.** The Switch passes the signed stick Y value through from libnx. `PadState::look_delta` already turns positive stick Y into negative Horizon pitch, as in the desktop input convention. The render loop subtracted that delta a second time. It now adds the delta and clamps to ±90°. Horizontal input and camera matrix conventions are preserved.
- **Held block.** The previous draw unprojected the projected hand into world space and drew it against terrain depth. Nearby terrain could cut through the block, and every hand face had the same upward normal. The hand now retains its model in camera space and uses the original Horizon hand projection at the framebuffer aspect ratio. A second pass loads world color and clears depth, with the hand using transformed face normals and its own shading. The existing normalized baked UVs, texture-array layers, triangle winding, back-face culling, and GLES coordinate conversion are preserved. The HUD draws after the hand.
- **Log.** The previous logger ignored directory errors, redirected stderr with unchecked `freopen`, and discarded diagnostics if that failed. The console run did not capture the original errno. The C entry now checks the SD mount and directories, opens a separate file with `fopen`, checks writes, flushes, and close, and keeps stderr as a fallback. It displays I/O failures on a console screen at exit. The file starts at C startup and records asset, player, world, renderer, presentation, save, shutdown, and result milestones.

Code changed in `build.py`, `client_entry.c`, and `horizon-client-switch/src/lib.rs`.

    source ferramentas/env.sh
    CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test5b_horizon_client_fix
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test5b_horizon_client_fix.nro -s

Rust release build and AArch64 linking passed. ELF inspection found zero undefined symbols or dynamic libraries. The NRO header and all 51 RomFS files matched the staged assets byte for byte, including the 14 textures. Local Naga validation and GLSL ES 320 emission passed for the two vertex and two fragment entry points. The eight previous NRO hashes match the recorded values.

- NRO `out/test5b_horizon_client_fix.nro` has SHA-256 `fb46c7b29dc1c0d0fb3928c43e00a462131d1237acd62e8d3d2563b953d5b04d`.
- Runtime log is `sdmc:/switch/horizon-tests/test5b_horizon_client_fix.log`. The existing prototype save directory is reused.
- On hardware, check that up looks up and down looks down, walk into a wall while holding a textured block, cycle the hotbar, and confirm the held block remains solid and correctly textured in front of the terrain. Save, exit with PLUS, and verify the log ends with `RESULT=0` and `RETURN 0`.
- User confirmed the Test 5B hardware checklist, including appearance, SD logging and clean exit.

## Test 6 — Ticket-driven streaming (hardware confirmed with stutter)

The Switch client imports the original `ChunkTickets` loading levels and walks candidate positions in Horizon's original spiral order. A radius of one maintains nine resident columns after a transition. Negative positions use `BlockPos::chunk` and Euclidean local coordinates. The prototype's Perlin terrain is evaluated in world coordinates. Edits stay in Horizon's palettes and are written through `RegionFile` to their signed Anvil region before eviction. Meshes are cached per resident column. An edit or membership change invalidates the affected neighboring meshes.

Saves use `sdmc:/switch/horizon/saves/switch_streaming_v1/`, separate from the approved Test 5B world. Both column and player records carry a format version. A truncated region aborts the transition before live columns are evicted. The last complete player record is kept as `level.dat.bak` when saving a new one, avoiding a rename over an existing destination on the SD filesystem.

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/test_streaming.py
    CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test6_horizon_streaming
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test6_horizon_streaming.nro -s

Host tests passed (13 passed, 1 ignored) including deterministic regeneration, signed-region edits through eviction and reopen, backup player recovery and failure without eviction. Rust release build and devkitA64 link passed. The ELF is AArch64 with no undefined symbols or dynamic libraries. The NRO header and all 51 RomFS files passed inspection. The nine preceding NRO hashes match and the build script refuses to replace hardware-confirmed artifacts.

- Artifact `out/test6_horizon_streaming.nro`, 8,198,777 bytes, SHA-256 `cbb60c20af878c499e05dd378afaa2179dd78ce2e23521569763b0f152ba20df`.
- Log `sdmc:/switch/horizon-tests/test6_horizon_streaming.log`. Look for `STREAM center=... resident=9` with nonzero `generated` and `evicted` when crossing chunks, then `read` when returning to an edited chunk. The log includes transition time in microseconds.
- Left stick moves, right stick looks, B jumps, ZR breaks, A/ZL places, L/R selects the block, X saves and PLUS exits.
- Place a distinctive block, move at least two chunk columns away, then return to check the edit. Save with X, exit with PLUS and reopen. Check that the edit and player state persist, and that the log ends with `RESULT=0` and `RETURN 0`.

Current limits are the 32-block prototype height and a fixed one-column view radius. The user reported microstutter at chunk borders. The supplied `test6_horizon_streaming.log` shows 19 in-play transitions with a 29.397 ms minimum, 41.052 ms median, and 100.906 ms maximum before meshing. The console kept nine columns resident, saved two departing columns, read edited columns on return, and exited with `RESULT=0`. This run began with `restored=false`.

## Test 6B — Asynchronous chunk transitions (confirmed on hardware)

One worker prepares columns and writes evicted snapshots without holding the live world lock. The render loop keeps its resident columns until the worker result is ready. It discards an obsolete view and retries a departure if its live revision changed during I/O. Manual saves and respawn wait for an in-flight request. Shutdown joins the worker before saving, so it cannot race a region write. The save format and directory are the same as Test 6.

Normal frames rebuild at most one column mesh, starting nearest the player, and reuse their staging allocation. Startup and explicit respawn still load synchronously. A column mesh and the per-frame terrain upload still run on the render thread. `STREAM_ASYNC` separates worker time from application time. `FRAME_TIMING` reports the maximum frame interval, number of intervals over 33 ms, and the slowest column mesh.

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/test_streaming.py
    CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test6b_horizon_streaming_async
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test6b_horizon_streaming_async.nro -s

Host tests passed (14 passed, 1 ignored), including a worker saving an old snapshot while an edit remains live, then retrying and restoring the latest edit. The WSL release build and AArch64 link passed. The NRO header, static ELF and RomFS passed inspection. All ten preceding NRO hashes match.

- Artifact `out/test6b_horizon_streaming_async.nro`, SHA-256 `04f188fbdb39831bb0c040f1a6667c57af7f5351083d4d92a1b3d4b601f5cabc`.
- Log `sdmc:/switch/horizon-tests/test6b_horizon_streaming_async.log`.
- The user crossed chunk borders repeatedly on a physical Switch and reported that the microstutter was effectively gone. The supplied `test6b_horizon_streaming_async.log` records 12 crossings, nine resident columns, and a clean exit with `RESULT=0` and `RETURN 0`.
- Test 6 blocked on each transition for 29.397–100.906 ms before meshing, with a 41.052 ms median. Test 6B applied each result in 0.053–0.075 ms on the render thread while the worker took 38.186–69.016 ms. Test 6B presented 3,232 frames and recorded three intervals above 33 ms, a maximum of 78.632 ms, and a maximum column mesh time of 3.171 ms. Test 6 lacks frame interval telemetry. Test 6B has no phase breakdown for those three peaks. Per-frame terrain uploads remain on the render thread.
- The Test 6B run restored the player and read one persisted column on startup. All 12 crossings loaded and evicted three columns each. This run had no block edits or dirty writes. Offline tests cover edited columns through eviction and reopening, including edits made while the worker is writing an older snapshot.

## Test 6C — Client ran on hardware; automatic capture failed

The Test 6B client now mirrors application records to an optional nxlink callback. A bounded queue hands records to a worker that owns the socket, keeping network I/O off the render thread. The SD log remains available for direct launches and connection failures. The frame timing record now includes interval count and total elapsed time. This uses the existing streaming world and save format.

    source ferramentas/env.sh
    python3 ferramentas/switch-tests/test_hardware_capture.py
    CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test6c_horizon_nxlink_log

The loopback test uses the SDK nxlink executable and the capture sender. It verified that a complete session returns 0 and both a truncated session and a dropped record return 2, with separate capture files. The Rust release build and AArch64 link passed. The ELF has no undefined symbols, external dynamic dependencies, or dynamic interpreter. The NRO and ASET bounds passed inspection, and the RomFS is byte-identical to Test 6B. All 11 previous NRO hashes match.

- Artifact `out/test6c_horizon_nxlink_log.nro`, 8,264,313 bytes, SHA-256 `70497726858b0af6993e610c4e7c39292db16cf84a5741abfadd43b379dc110d`.
- Build, artifact inspection, and capture tests are recorded in `out/test6c_horizon_nxlink_log.build.log`, `out/test6c_horizon_nxlink_log.offline.log`, and `out/test6c_horizon_nxlink_log.tests.log`.
- With nxlink active in Sphaira at 192.168.1.5, run `python ferramentas/switch-tests/run_hardware_test.py test6c_horizon_nxlink_log` from PowerShell at the repository root. The receiver listens on TCP 28771 on the Windows LAN interface and invokes the SDK's WSL nxlink for upload. When the callback is reachable directly from WSL, run `bash ferramentas/switch-tests/run_hardware_test.sh test6c_horizon_nxlink_log` there.
- The runner streams the application's log and saves a timestamped copy under `out/hardware-logs/`. Walk across several chunk borders and exit with PLUS. A complete capture contains BEGIN, frame and streaming records, `RESULT=0`, `RETURN 0`, and END. The SD copy is `sdmc:/switch/horizon-tests/test6c_horizon_nxlink_log.log`.
- The user launched the NRO and exited with PLUS on hardware. The terminal waited for a callback until interrupted. The SD log records `WARN nxlink capture incomplete: nxlink callback was unreachable` after `RESULT=0` and `RETURN 0`. It reports 2,351 frames, three block breaks, five placements, and one saved chunk. Capture work is deferred at the user's request. The SDK uploader remains usable to send subsequent probes, whose diagnostics go to the SD card.

Test 6B remains the stable client baseline. The next hardware milestone is the original vanilla generator, exercised in isolation before integration with the async streaming worker.

## Test 7 — Isolated Java 26.3 terrain column (confirmed on hardware)

`horizon-worldgen-switch` uses `BlockRegistry::from_facts_json`, `GeneratorData::load`, the normal overworld preset via `OverworldGenerator::new`, and `GeneratorWorker::generate_proto`. It runs the original pre-light terrain pipeline with biome sampling, noise, aquifers, surface rules, structure starts and terrain adaptation, and carvers. The probe holds one generated column at a time and runs generation on one dedicated Rust thread. The main thread keeps the libnx text console responsive and mirrors progress to the SD log. This stage has no playable client, decoration pass, lighting, or game saves.

`horizon-prepare` verified the 26.3 client and server JARs against `research-lock.json`. Missing Linux libraries from the Windows install were staged separately and checked against the version manifest. Eclipse Temurin JDK 25 was unpacked to a temporary WSL directory for the original Java exporters. Preparation finished on the WSL filesystem. `prepare_worldgen_probe.py` runs the existing `VanillaChunkGenerationOracle` at terrain status, validates its dimensions against the extracted 26.3 dimension data and stages the prepared registry, worldgen bundle and reference in the ignored output tree. The NRO reads these from RomFS.

Host probe passed for seed 12345, chunk (-2, 3). The dimension spans Y −64 through 319, with 24 sections and sea level 63. Both runs matched the Java fingerprints for all 98,304 block states and 1,536 quart-biome cells. The column contains ocean and lush caves biome cells. The test checked all six heightmaps against vertical block scans. NBT round trip preserved the positions, block and biome palettes, and heightmaps in 21,604 bytes. This snapshot tests the terrain data codecs in memory.

| WSL release probe | Measured |
| --- | ---: |
| Load registry and generator | 1,499 ms |
| First generation | 1,439 ms |
| Structure stage on the first generation | 1,425 ms |
| Repeat with the same generator and a fresh worker | 15 ms |
| Tracked heap for one finished proto column | 32,176 bytes |
| Peak RSS for the whole process | 113,384 KiB |

The warm run shares generator caches with the first run, including structure data. Its time describes this specific repeated column. Cold structure work needs a separate budget before connecting the generator to the render client. On Switch the probe logs process memory snapshots through libnx and stage timings. Those measurements are pending.

The first AArch64 link exposed a missing `pread` symbol. The installed newlib headers declare it, but the linker could not resolve it. `horizon-core/src/prepared_files.rs` now selects the existing mutex-protected seek/read path for Horizon OS. Linux and Windows keep their existing positional reads. All five host tests for prepared-file reads passed after the change. The Rust release build and devkitA64 link passed. The ELF has zero undefined symbols and no external dynamic dependencies. The NRO and asset bounds are valid, all 1,543 RomFS files match the staged data, and all 12 previous NRO hashes match.

- Artifact `out/test7_vanilla_worldgen.nro`, 50,293,069 bytes, SHA-256 `38857a3a7648d7be6a5dbaaeae6284f67a2081f5e078af0f9f1af3785dbfd729`.
- Source changes are in `horizon-worldgen-switch/`, `prepare_worldgen_probe.py`, `build.py`, `client_entry.c`, and `crates/horizon-core/src/prepared_files.rs`. The network logger build is restricted to Test 6C.
- See `out/test7_vanilla_worldgen.host.log`, `out/test7_vanilla_worldgen.build.log`, `out/test7_vanilla_worldgen.offline.log`, and `out/preparation/vanilla-fixture.log`.

    source ferramentas/env.sh
    export JAVA_HOME=/tmp/horizon-switch-jdk25/jdk
    export PATH="$JAVA_HOME/bin:$PATH"
    prep=$(cat ferramentas/switch-tests/out/preparation/linux-prep-root.txt)
    python3 ferramentas/switch-tests/prepare_worldgen_probe.py --resources "$prep/.local/horizon-client/26.3"
    cargo build --release --offline --manifest-path ferramentas/switch-tests/horizon-worldgen-switch/Cargo.toml
    /usr/bin/time -f 'HOST wall_seconds=%e peak_rss_kib=%M' /tmp/horizon-switch-target/release/horizon-switch-worldgen ferramentas/switch-tests/out/test7-worldgen-romfs
    CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test7_vanilla_worldgen
    ferramentas/devkitpro/tools/bin/nxlink -a 192.168.1.5 ferramentas/switch-tests/out/test7_vanilla_worldgen.nro

The physical Switch execution matched the Java reference fingerprints, heightmaps, NBT and determinism (init 59,761 ms, first column 1,180 ms, repeat 64 ms, RESULT=0). The reported 3,339,825,152 bytes is kernel process-memory accounting, including mapped heap. libnx can allocate the available heap at startup, so the number does not identify the live allocations attributable to generation. See [libnx heap initialization](https://github.com/switchbrew/libnx/blob/master/nx/source/runtime/init.c) and [Switchbrew GetInfo](https://switchbrew.org/wiki/SVC#InfoType).

## Test 8 — Vanilla Java 26.3 world generation with async streaming client (awaiting hardware validation)

Connects the validated Minecraft Java 26.3 vanilla generator (`OverworldGenerator`, `GeneratorWorker::generate_proto`) to the playable first-person client with asynchronous chunk streaming (`StreamWorker`).

Key architectural points:
- **Full vanilla world height**: Y = -64 through 319 (384 blocks total height, 24 vertical 16-block sections per chunk column).
- **Asynchronous worker**: Terrain generation (`vanilla::generate`), serialization/deserialization, and Anvil region I/O run in a background worker thread with 4 MiB stack (`"Switch chunk storage"`), preventing main render loop stalls during chunk transitions.
- **Section-based meshing**: Mesh generation is budgeted per 16-block section with frustum and opaque-face culling. The original Test 8 repacked visible sections each frame and skipped any that exceeded its staging capacity.
- **Local generator execution**: Runs 100% locally on the Nintendo Switch AArch64 hardware using the embedded RomFS asset and data bundle (1,669 files, 44 MiB). No server or external PC required.
- **Deterministic seed & reference parity**: Fixed seed 12345, verified offline against the Test 7 Java reference fingerprints for blocks and biomes across all 24 sections.
- **Experimental isolated save**: Saves to `sdmc:/switch/horizon/saves/switch_vanilla_test8/`, keeping previous prototype saves (`switch_streaming_v1/`) completely intact.
- **Interactive gameplay systems**: Preserves 60 FPS OpenGL ES 3.2 rendering via `wgpu-hal`, first-person camera with view bobbing and FOV calculations, 20 TPS physics tick loop, collision resolution against vanilla VoxelShapes, voxel raycast, block breaking (ZR / D-Pad Left), block placing (A / ZL), 9-slot hotbar (L / R), manual save (X) and respawn (Y / fall below Y = -76).
- **SD logging**: Full execution metrics logged to `sdmc:/switch/horizon-tests/test8_vanilla_streaming.log`.

Offline validation:
- Host vanilla streaming tests (`python3 ferramentas/switch-tests/test_streaming.py --vanilla`): 11 tests passed in 2.41s, verifying 3x3 initial column generation, oracle state/biome hash match, block edits, dirty section flags, and persistence through eviction and reloading.
- Host legacy streaming tests (`python3 ferramentas/switch-tests/test_streaming.py`): 14 tests passed, 1 ignored.
- AArch64 binary inspection: Static ELF linked cleanly with 0 undefined symbols. Valid `NRO0` header.
- NRO artifact: `ferramentas/switch-tests/out/test8_vanilla_streaming.nro` (57,885,357 bytes, SHA-256 `106db3017167533a076c0c781f98cafa809767cea44ed8e9ab4876eef9f3fc72`).

Pending hardware items:
- The supplied hardware log and photograph show a black terrain-shaped region and a dark crosshair. The run presented 10,982 frames, crossed chunks, recorded 14 breaks and six placements, saved two columns and exited with RESULT=0. It did not test reopening. The upload cap excluded visible sections in 111 frames. The cause of the black pixels remains under investigation.
- The cause of the ~60s cold startup observed in Test 7 remains to be isolated with hardware timing data.

How to deploy:
```sh
source ferramentas/env.sh
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8_vanilla_streaming.nro
```

## Test 8B — GLES diagnostics and section uploads

The offscreen test runs the client WGSL through Naga's GLES backend and draws with Mesa llvmpipe. It exercises the production vertex layout, a 42-layer RGBA8 atlas uploaded from a mapped pixel unpack buffer, UV selection, nearest sampling, depth testing, back-face culling, and an overlay draw after clearing depth. The client uses non-indexed triangles. The test read `[32, 192, 64, 255]` for solid color, `[200, 100, 40, 255]` for the unlit texture, `[100, 50, 20, 255]` for lit terrain, and `[255, 255, 255, 255]` for the HUD's white layer. The Linux test uses a synthetic scene and direct GLES calls. Its passing pixels leave the Switch-specific source of black pixels unresolved.

The diagnostic NRO reads back all 42 actual atlas layers and compares their bytes against the decoded source. `GLES_ATLAS` reports the number of mismatches and the first affected layer, framebuffer status and GL error. `GLES_PIXELS` samples the swapchain before presentation on frames 1 and 180. The samples cover the center, hotbar region and lower scene. Together these distinguish atlas corruption from the rendered framebuffer and the subsequent presentation step.

The client now keeps uploaded terrain buffers per section and queues the nearest pending revision. The 6 MiB limit applies to staging for the frame, with larger sections resuming on the next frame. Complete sections draw directly from resident buffers, and leaving the 3×3 resident window releases those buffers. Until a replacement finishes uploading, its section can be briefly absent. The old path could omit visible sections every frame if they did not fit the staging budget.

- Offline validation passed with 24 client tests and two ignored tests. The legacy streaming suite passed with 14 tests and one ignored. The opt-in block-table timing test passed separately.
- Rust release and AArch64 link passed. The ELF has zero undefined symbols and no dynamic dependencies. NRO and ASET bounds passed, and the RomFS is byte-identical to Test 8. All 14 prior NRO hashes match.
- `out/test8b_vanilla_render_fix.nro` is 57,909,933 bytes. SHA-256 `628a399b73905d7c4b70f82f3b2ada639fb519d17be9c4fe08f68ad65a6af540`.
- Diagnostic and test sources are `horizon-client-switch/src/pixel_test.rs`, `mesh_upload.rs`, `lib.rs`, `block_table_probe.rs`, and `vendor/wgpu-hal/src/gles/horizon_egl.rs`. The build target is registered in `build.py`.

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test8b_vanilla_render_fix
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8b_vanilla_render_fix.nro
```

On Switch, wait for the world to render, look at the ground and hotbar, cross a chunk border, and exit with PLUS. Copy `sdmc:/switch/horizon-tests/test8b_vanilla_render_fix.log` back to the project. The build uses the Test 8 experimental save.

The WSL timing probe read the collision, property and support tables in 33, 51 and 32 ms respectively. NBT decoding plus registry validation took 52, 91 and 48 ms. Constructing the collision table from the decoded data took 8 ms. The three files total 21,818,500 bytes. The timings are from the host and do not attribute the 55.7 seconds observed on Switch. Keep the table validation and data intact until equivalent on-device measurements separate I/O, decoding and runtime table construction.

## Test 8C — Controlled GLES render and readback

The 8B console log reports a byte-for-byte match for all 42 atlas layers. The center, hotbar and lower-scene samples were opaque black on frames 1 and 180. Those coordinates did not include a controlled clear color. The 8B photograph still shows blue sky, so the three samples alone cannot distinguish black draw output from a readback or presentation problem. The 8B session restored the player, registered one block break and five placements, saved one column and exited with `RESULT=0`.

The 6B, 8 and 8B NROs contain byte-identical production WGSL (SHA-256 `e50aeb676222301ff1f9fd4d6e3053d2dcba6140f4445f46e7541fd8cdd0edd0`). The 6B working path used the prototype voxel enum and per-column CPU meshes from 32-block Perlin terrain. Test 8 uses registry-backed vanilla sections, maps original block states to baked visuals by name with a stone fallback, and caches one mesh per 16-block section. Both paths resolve a model quad's sprite through the layer map and submit non-indexed triangles with the same shader inputs. The hardware logs identify the same NV120 adapter, EGL version and Depth24Plus path, while models and atlas layers grew from 10 and 15 to 33 and 42. Assets moved from SD to RomFS, and Test 8 adds section frustum culling and a bounded combined upload. Test 8B replaces that combined terrain upload with resident section buffers, yet the symptom persists. The earlier 6B source snapshot is absent from the checked-in history, so the comparison uses the original binary, hardware logs and the surviving legacy code path.

The 8C startup submits one clear-only frame followed by five panels using the game's vertex buffer, atlas, camera uniform, pipeline layout and two render passes. It temporarily writes a two-color texture to the white atlas layer for the panel frame and restores that layer before gameplay. The SD log records `GLES_WORLD` after the terrain pass, `GLES_PRE` after the HUD pass, and `GLES_POST` after the Y-flipped blit and before EGL swaps the window. It also records EGL window dimensions and framebuffer ID, status, selected read buffer, coordinates, GL errors and RGBA values. The probe waits for GL work and temporarily resets pixel-pack state.

Expected frame 1 samples are sky blue `[32, 96, 224, 255]`. Frame 2 sample order is upper sky, lower sky, solid, sampled texture, production lighting, solid HUD, direct texel, center and hotbar. The five panel colors in order are green `[64, 191, 64, 255]`, orange `[224, 64, 32, 255]`, shaded orange `[112, 32, 16, 255]`, purple `[191, 64, 191, 255]` and cyan `[32, 192, 224, 255]`. The HUD panel appears after the HUD pass. Frames 3 and 180 sample the normal scene.

The shared readback helper passed on llvmpipe with a real EGL pbuffer, an RGBA8 renderbuffer, a known blue clear color and an asymmetric marker. It returned matching pre- and post-blit colors at flipped coordinates and restored the previous read framebuffer and pixel-pack state. The same Naga fragment entries and panel vertices returned the expected colors in the host pixel test. The Linux test issues direct GLES calls. The Switch build exercises the client HAL command encoder and physical driver.

- The Linux client suite passed 24 tests with two ignored, including the controlled GLES cases and the vanilla streaming suite. The legacy streaming suite passed 14 tests with one ignored. Logs are in `out/test8c.gles.log` and `out/test8c.legacy.log`.
- Rust release compilation and AArch64 link passed with zero undefined symbols and no dynamic dependencies. NRO0 and ASET bounds passed. The RomFS matches Test 8 byte for byte, and all 15 earlier NRO hashes match. Verification is in `out/test8c_gles_pipeline_probe.offline.log`.
- `out/test8c_gles_pipeline_probe.nro` is 57,918,125 bytes. SHA-256 `10a81bdcc68ded6db72f39cacdeeee99992befb60a07d3ff2147e5bbadc4ca47`. The first failing stage is pending physical validation.

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test8c_gles_pipeline_probe
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8c_gles_pipeline_probe.nro
```

On Switch, allow the game to run for at least five seconds after the game view appears, then exit with PLUS and copy `sdmc:/switch/horizon-tests/test8c_gles_pipeline_probe.log`. Compare the known sky and panel samples first, then use the first stage with unexpected RGBA or a GL error to narrow the failure.

## Test 8D — GLES atlas access

On the physical Switch, 8C read the exact blue clear and both solid panels from the complete 1280×720 intermediate framebuffer with GL error 0. The unlit and lit atlas panels returned `[0, 0, 0, 255]`, and `textureLoad` returned `[0, 0, 0, 0]` before the HUD and window blit. The startup attachment readback matched all 42 original layers. The black output is isolated to the textured draw path. Attachment readback establishes texture contents, while a draw also needs the correct texture unit, complete mip state and working shader access. The EGL window size was zero, which skipped the post-blit probe. 8D retries before each sampled presentation and logs the libnx NWindow dimensions as a fallback.

The 6B and 8 logs report the same NV120 adapter, EGL 1.4, D2Array, Depth24Plus and HAL bind-group path. The production WGSL is byte-identical. In the checked-in renderer, WGSL bindings 0, 1 and 2 are the camera uniform, D2Array view and nearest sampler. The HAL maps them to separate namespaces, so the first texture should occupy GLES unit 0. The GLES view shares the texture object and covers the entire layer range. The queue applies mip base and maximum 0 and binds the sampler for `textureSample`; the `textureLoad` pipeline has no WGSL sampler mapping. The 6B log shows 15 layers and two animations, compared with 42 layers and three animations in Test 8. Its standalone client source snapshot is absent from Git. The larger array, the additional animated uploads and the vanilla model mapping are established differences. The relative effect of those differences on NV120 remains to be measured.

- Frame 2 keeps the original panels and adds a center panel for `textureLoad` at `(0, 0)`, layer 0, mip 0. This draw has no UV or layer vertex input. Its expected RGBA appears in `GLES_ATLAS_BIND`. Both load panels and the sample panels use the real atlas view. `GLES_ATLAS_PANEL` checks that the two-color pattern actually reached the last layer during that frame.
- `GLES_ATLAS_BIND` reports the linked program's active sampler uniform and unit, current program, expected and actual texture object, view ranges, mip parameters, filters and level dimensions. It runs after the frame 2 fixed load and the frame 3 normal terrain/HUD draw.
- After frame 3, `GLES_ATLAS_NATIVE` uses handwritten GLSL ES and draws into a separate RGBA8 framebuffer. It samples the same atlas object and control textures with both `texelFetch` and `texture`. Cases compare a 2D texture, immutable 15- and 42-layer arrays and a mutable 42-layer array. Array checks cover layers 0, 14, 15, 31, 32 and the last available layer. Unit 0 is tested with and without the production sampler object, and unit 1 is tested with it. The production atlas is also tested with nearest filtering on the texture object. Its parameters are restored after each case. Frame 4 and frame 180 sample normal gameplay.

Compare the fixed and attribute-driven load panels first. A fixed-load pass with an attribute-driven failure narrows the fault to UV or layer inputs. If native sampling of the same atlas passes, inspect the HAL binding snapshot and the difference between the generated and handwritten GLSL. If the native arrays fail but the 2D control passes, compare the array depths, storage modes, units and sampler settings. A pass for the new 42-layer control with a failure for the production atlas narrows the next check to the production object's state and upload history. Record the first divergent case before changing the production render path.

- The Linux EGL test uses Mesa llvmpipe. The shared native probe returned zero mismatches and GL errors across the texture cases. The Naga-generated fixed load returned the independent expected color `[88, 144, 200, 255]`. The client suite passed 24 tests with two ignored, including the vanilla oracle and save regression. Legacy streaming passed 14 tests with one ignored. Logs are in `out/test8d.gles.log` and `out/test8d.legacy.log`.
- Rust release and AArch64 link passed with zero undefined symbols and no dynamic dependencies. NRO and ASET bounds passed, and the RomFS is byte-identical to Test 8. All 16 earlier NRO hashes match, including the physical 8C artifact. See `out/test8d_gles_atlas_probe.offline.log`.
- Artifact `out/test8d_gles_atlas_probe.nro`, 57,938,605 bytes. SHA-256 `f51d9cbb672a0622821c4ecdc9b77bb6dac4310e25c4927da02e011680b203fc`. Physical validation is pending.

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test8d_gles_atlas_probe
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8d_gles_atlas_probe.nro
```

On Switch, wait at least five seconds after the game view appears, then press PLUS and copy `sdmc:/switch/horizon-tests/test8d_gles_atlas_probe.log`. The startup panels last one frame and the SD log records their colors.

## Test 8E — Explicit GLES 2D array allocation

The physical 8D log records `target=0x9009 bound=None expected_texture=NativeTexture(1)` for both the fixed fetch and normal terrain program. `0x9009` is `GL_TEXTURE_CUBE_MAP_ARRAY`. The linked `sampler2DArray` uniform selects unit 0 and reads its `GL_TEXTURE_2D_ARRAY` binding, which is empty. The HAL binds the recorded native target. The render-pass end and queue submission leave texture bindings intact, so the snapshot still describes the state used by those draws. Layer attachment readback works with cube array layer-faces and therefore matched all the uploaded bytes.

The generic GLES texture constructor infers cube compatibility from square dimensions, a sample count of one and a multiple of six layers. The 15-layer 6B atlas selects `GL_TEXTURE_2D_ARRAY`. The 42-layer Test 8 atlas selects cube-array storage. Creating the D2Array view clones the same native object and target, and the bind group carries that target to the queue. Upload and attachment readback used the allocated object successfully. Destruction occurs after the draw loop. The 8D native diagnostic never reallocated the atlas. It attempted to bind that same cube-array name to a different target, which GLES rejects with `GL_INVALID_OPERATION`. The revised diagnostic checks the native target first and changes parameters only on its own control textures. [OpenGL ES 3.2 specification, §§ 8.1 and 9.4](https://registry.khronos.org/OpenGL/specs/es/3.2/es_spec_3.2.pdf)

The client now explicitly allocates `GL_TEXTURE_2D_ARRAY` through the existing HAL allocator. The texture remains RGBA8 with its original pixels, layer order and mip level 0. The sampler remains `GL_NEAREST_MIPMAP_NEAREST` (`9984`). With base and max both 0, the complete mip range is level 0 alone. The production WGSL and shader bindings remain the same. The 8D hardware controls already sampled the single-mip arrays successfully with this sampler. [OpenGL ES 3.2 specification, §§ 8.14.3 and 8.17](https://registry.khronos.org/OpenGL/specs/es/3.2/es_spec_3.2.pdf)

The Linux GLES regression creates both sizes through the generic HAL allocator, verifies the old target selection and reproduces `GL_INVALID_OPERATION` when rebinding the 42-layer cube array as a 2D array. It then allocates the same descriptor explicitly as a 2D array and verifies the actual native texture with Naga-generated sampling, fixed fetch and direct GLES controls.

Frame 2 adds a bottom-center panel at the last `GLES_WORLD`/`GLES_PRE`/`GLES_POST` sample. It samples a separate 42-layer RGBA8 control uploaded via the HAL staging encoder, using the same pipeline, sampler, view descriptor and bind-group layout as the production atlas. Expected RGBA is `[92, 212, 56, 255]`. The center panel reads the original atlas layer 0 and logs its expected color. The texture, lit and high-layer fetch panels retain their expected orange, shaded orange and cyan. `GLES_ATLAS_BIND` should show target `0x8c1a`, a matching bound and expected texture, and mip range 0 through 0. Normal frames should show the original textures on terrain and the hotbar. The 8D NWindow fallback already confirmed correct post-blit reads at 1280×720 on the console.

- The Linux client suite passed 24 tests with two ignored, including the GLES regression, vanilla oracle and save coverage. Legacy streaming passed 14 tests with one ignored. See `out/test8e.gles.log` and `out/test8e.legacy.log`.
- Rust release and AArch64 link passed with zero undefined symbols and no dynamic dependencies. The NRO and ASET ranges are valid. The production WGSL matches 8D byte for byte, the RomFS matches Test 8, and all 17 earlier NRO hashes match. See `out/test8e_gles_atlas_fix.offline.log`.
- Artifact `out/test8e_gles_atlas_fix.nro`, 57,946,797 bytes, SHA-256 `78554fb987fd8d81116d0c0c69bfe2d4d6ab1be999c3629dc90560bb1dc338c7`.

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test8e_gles_atlas_fix
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8e_gles_atlas_fix.nro
```

On Switch, inspect the terrain, held item and hotbar, cross a chunk boundary, and press PLUS. Copy `sdmc:/switch/horizon-tests/test8e_gles_atlas_fix.log` to the project. The diagnostic panels last a single frame and their sampled colors are recorded on the SD card.

The user approved 8E on physical Switch. `test8e_gles_atlas_fix.log` records a matching texture object on target `0x8c1a`, mip 0, 42 layers, zero native probe failures and the expected post-blit control pixel `[92, 212, 56, 255]`. The run presented 5,282 frames, broke eight blocks, placed eleven, saved three chunks and exited with `RESULT=0` and `RETURN 0`. It recorded 25 intervals over 33 ms and a maximum column mesh time of 6,878 µs. The approved 8E SHA-256 matches the recorded hash. The user reported opaque blue water.

## Test 8F — Vanilla water rendering

The Switch prototype baked a fallback cube from the water particle sprite. Its terrain fragment used a cutout threshold and returned alpha 1. In the desktop renderer, the fluid tessellator computes weighted surface corners and face occlusion from registry states, uses the shared flow vector for top UVs, and chooses still, flow or overlay sprites. Water goes into the translucent pass and retains the sprite alpha.

8F extracts the desktop geometry into `crates/horizon-client-render/src/mesher/fluid_geometry.rs` and uses it from both renderers. The Switch adapter reads the original fluid states, reuses `horizon-world` flow and uses the stored quart biome water tint. The terrain mesh emits water into a separate range. The existing upload budget stages the same resident buffer, with opaque geometry drawn first and the visible water ranges submitted back-to-front by section. Water uses standard alpha blending, depth testing and depth writing like the desktop pass. It renders before the hand and HUD. The layer size is 32×32 to preserve the original flow frames. Nearest expansion of the 16×16 sprites preserves their texels and alpha under the existing nearest sampling. The approved 8E atlas allocation fix is used for this array.

The Switch adapter uses its existing full-block occlusion and model coverage, plus the leaves overlay. The flow calculation uses the Switch block motion flags and full-block support for neighbor queries. Fine occlusion for unsupported partial and waterlogged block models, blended biome borders, per-quad sorting inside sections, underwater camera effects and fluid simulation remain outside this milestone. Source, flowing and falling water are read from the persisted registry states. The save format and path stay the same. Water mesh invalidation now includes diagonal chunks and adjoining vertical sections at edited edges.

The host tests cover an isolated source, sloped surface geometry and side UVs, hidden faces beside solids and other water, and translucent water over opaque geometry with depth occlusion. The streaming regression saves and reloads exact source, flowing and falling state IDs alongside the existing Java terrain and edit checks. The release logs frame and mesh timings on Switch for comparison with 8E.

- The Switch client suite passed 26 tests with two ignored. Legacy streaming passed 14 tests with one ignored. The regular desktop mesher suite passed 55 tests with 17 ignored. Four opt-in desktop reuse tests stopped at resource loading because the local desktop bundle lacks `horizon-resources.json`. Logs are in `out/test8f.gles.log`, `out/test8f.legacy.log`, `out/test8f.desktop.log` and `out/test8f.desktop-prepared.log`.
- Rust release and AArch64 link passed with zero undefined symbols and no dynamic dependencies. The NRO and ASET ranges are valid. All 1,672 RomFS files match staging. Relative to 8E, the RomFS adds `water_flow.png`, its animation metadata, and `water_overlay.png`, and updates the manifest. The original opaque and held-item WGSL matches 8E byte for byte. All 18 earlier NRO hashes match.
- Artifact `out/test8f_vanilla_water_fix.nro`, 57,950,497 bytes. SHA-256 `e91c33a629e3f14f26e4219ace680636f66447b44e422916e6cf73b6ea92d6e6`. Physical validation is pending.

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test8f_vanilla_water_fix
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8f_vanilla_water_fix.nro
```

On Switch, look at shallow water over blocks, shoreline sides and flowing water. Check that the bottom remains visible, that nearby terrain occludes water, and that the held item and hotbar render normally. Cross a chunk boundary along the shore, press PLUS and copy `sdmc:/switch/horizon-tests/test8f_vanilla_water_fix.log`. The build opens the same `switch_vanilla_test8` save directory.

The supplied 8F console log confirms a clean exit after 6,851 frames, six breaks, 43 placements and one saved chunk. The restored pose was `(65.73, 48.00, -16.11)` with eye Y 49.62. The log records sea level 63 and omits the fluid at the player's coordinates. Water appearance away from the submerged region remains pending.

## Test 9 — Vanilla first-person polish

- The Switch now uses the original first-person hand anchor, WHACK swing, equip heights and per-model right-hand display transforms. The visible stack changes at the bottom of the equip motion. Attack starts a six-tick swing and successful placement drops and raises the hand. The fixed 20 Hz state is interpolated at render time. Both walking and sprinting use the existing Horizon camera bob, including the held block, with a separate depth pass for the hand. Supported hotbar entries are block items. Specialized tool, food, ranged and off-hand poses require the desktop item components and models.
- The camera retains the original yaw/pitch convention, smooth eye-height interpolation and sprint FOV. Stick look is applied directly each frame while position is interpolated. The controller settings accept `fov:70` in `sdmc:/switch/horizon/horizon-controller.txt`, with values clamped to 30–110. Add the setting to an existing file to override the default. A low ceiling keeps the player crouched until standing fits.
- The Switch loads the saved area before checking the player bounding box. Valid poses are kept, including water, midair and positions where only crouching fits. Intersecting or out-of-height positions seek nearby supported dry terrain. If the local search fails, recovery tries the original world spawn from the vanilla generator. Y requests a return to the world spawn. The save format and directory remain compatible. An unreadable or malformed save fails explicitly.
- Water depth and eye submersion come from the registered fluid and its height in loaded blocks. The original `living_fluid::travel_in_water` supplies acceleration, drag, gravity and ledge escape. B rises and R3 toggles sneak/sink. Camera fog blends to the biome water fog color and uses the desktop's starting range of −8 to 24 blocks. The water surface and explicit GLES 2D array allocation from 8F remain in use. Swim posture, currents on the player, breath, lava travel and fluid simulation remain outside this milestone.
- Terrain uses the original Overworld face factors of 1.0, 0.5, 0.8 and 0.6. The held block uses the same two-light diffuse rule as the desktop entity shader. This milestone covers directional shading. Propagated block light, smooth ambient occlusion, shadows and underwater effects such as distortion still need integration.
- 8F reached its first frame in 63,664 ms, including 58,534 ms for worldgen tables and 58,390 ms for the overlapping block-table stage. Its CPU mesh peak was 16,911,544 bytes and staging reached 598,280 bytes. The 791.4 s gap in the frame log distorts aggregate timing. Test 9 counts gaps above one second separately and times asset baking. The renderer still budgets resident section uploads and reuses frame buffers. Shorter startup and console frame time require hardware measurements.

- PASS OFFLINE for Rust release, AArch64 link, zero undefined symbols, NRO and ASET bounds. The ELF includes the new hand and water shader entries. The complete 1,672-file RomFS matches 8F byte for byte. The 19 prior artifact hashes match.
- PASS OFFLINE for 30 Switch-client tests (two ignored), 14 legacy streaming tests (one ignored), and 17 desktop first-person tests. The desktop run used `--no-default-features` because this WSL installation lacks the `pkg-config` command required for the default libudev controller build. The separate opt-in block-table probe also passed. Its three prepared files total 21,818,500 bytes. In the WSL debug run, reads took 69–115 ms per file and decode plus validation took 451–806 ms per file. This is a host baseline for the on-console load, with no timing claim for Test 9 hardware. Logs are in `out/test9.gles.log`, `out/test9.legacy.log`, `out/test9.desktop.log` and `out/test9.block-tables.log`.
- AWAITING HARDWARE VALIDATION for `out/test9_vanilla_first_person_polish.nro` (57,966,881 bytes). SHA-256 `cdcfa680ce8b76710655762d0e6a2a337727d59a3b41120797210a157104a9cb`.

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test9_vanilla_first_person_polish
ferramentas/devkitpro/tools/bin/nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test9_vanilla_first_person_polish.nro
```

On Switch, compare standing, walking, sprinting, crouching, jumping and landing. Change hotbar slots, break and place blocks next to a wall, and check the framing and animation of the held block. Compare water from the shore and underwater, ascend with B, and check that fog fades when the eyes leave the water. Cross a chunk boundary, save with X, exit with PLUS and reopen to verify pose and chunk persistence. Copy `sdmc:/switch/horizon-tests/test9_vanilla_first_person_polish.log` to the project.

