# Current Switch runtime checkpoint

Date: 2026-11-05. Lead: Codex. Active Alethe profile: base (existing user configuration).
Alethe orchestration: C:/Users/Pc vinw/AppData/Roaming/Alethe/orchestrations/20261105-171345-horizon-switch-std-abi
Provider: Antigravity 1.5.1, existing Google subscription authentication. No API key configured.
Global policy and profile were not changed. PATH correction applies to invocation processes only.
Windows CLI orchestration, WSL compilation. WSL cannot launch the host Python executable directly (Exec format error).

Preserved tests: 1 OpenGL, 2A Rust alloc, 2B Rust GLES, 2C C POSIX, 2C Rust std, 3 Horizon Integration, 4 Horizon Renderer.
Their NRO SHA-256 digests match results.json; all 7 have passed on real Nintendo Switch hardware (user_confirmed_pass).

Runtime status:
- Local libc fork: ferramentas/switch-tests/vendor/libc, upstream 0.2.191, original licenses retained.
- Specific AArch64/newlib branch (src/unix/newlib/switch.rs); Nintendo 3DS horizon/mod.rs remains byte-identical.
- Types/layouts measured by runtime-audit/measure_abi.py against SDK C headers.
- 334 C measurements plus Mutex/Condvar/rwlock initializer bytes, enforced by 163 compile-time assertions in abi_assertions.rs.
- libc and std compile cleanly with the custom target and explicit Cargo configuration patch.
- Link succeeds with zero undefined symbols: horizon_switch_sysconf / sysconf (4096 B page size matching Horizon OS 0x1000 granularity) and getrandom (backed by libnx randomGet CSPRNG) are implemented in std_entry.c, horizon_entry.c, and renderer_entry.c.
- Produced and hardware-validated out/test2c_rust_std.nro (379,077 bytes, SHA-256 1b676f7751c537ec3038cc8cc80d378069dd34e2e6c19c9a68558c37d298e750) -> user_confirmed_pass.
- Produced and hardware-validated out/test3_horizon_integration.elf (12,750,144 bytes) and out/test3_horizon_integration.nro (7,260,357 bytes, SHA-256 9a438f2fa8afbf2deb9682f4ee217115d79e241ab3555494c0310600199f5204) -> user_confirmed_pass (hardware log saved to out/test3_horizon_integration.hardware.log: renderer='NV120', OpenGL ES 3.2 Mesa 20.1.0-rc3, 284 frames, 95 server ticks, 10 mesh generations, RESULT=0).
- Produced and hardware-validated out/test4_horizon_renderer.elf (13,601,304 bytes, SHA-256 8dc1ac840e8c13c0e12a9e0f8596afff0d6c3167c5d15ce2ff1523c10bebce63) and out/test4_horizon_renderer.nro (7,858,809 bytes, SHA-256 373792bffbbe82e4f8aa12a34380d3222fcfb99214b2022b1c9cadd0850bd0cf) -> user_confirmed_pass (frames_rendered=2000, server_ticks=667, mesh_generations=52, RESULT=0, RETURN 0).

Useful files:
runtime-audit/patch_libc.py, abi.json, abi_initializers.json, generate_assertions.py,
abi_assertions.rs, probe.rs, cargo-switch.toml, aarch64-nintendo-switch-newlib.json, std_entry.c,
horizon_entry.c, horizon-integration/Cargo.toml, horizon-integration/cargo-switch.toml, horizon-integration/src/lib.rs,
renderer_entry.c, prepare_switch_assets.py, vendor/wgpu-hal/src/gles/horizon_egl.rs,
horizon-renderer/Cargo.toml, horizon-renderer/cargo-switch.toml, horizon-renderer/src/lib.rs.
out/test4_horizon_renderer.build.log is the latest successful build/link/NRO log for Test 4.

Reproduce inside WSL from repository root:
    source ferramentas/env.sh
    python3 ferramentas/switch-tests/build.py test4_horizon_renderer

Next:
Tests 4 and 5 ran on hardware. Test 5 confirmed movement, collision, editing, save persistence, and clean exit. It exposed inverted vertical look, held-block distortion, and a missing SD log.

- Recovered the existing Test 5 crate, build configuration, and C entry point. The 41 original Rust errors came from mismatches in camera, view bob, FOV, cooldown construction, and collision APIs.
- Fixed the calls in `horizon-client-switch/src/lib.rs`. The seven previously validated NROs match their recorded hashes.
- Reproduce with `source ferramentas/env.sh` followed by `CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test5_horizon_client`.
- Test 5 NRO SHA-256 is `fee9b5864cc65f3e21b5c65d6c3afa2b5d015b0000ee50634e1eaf29aa3ed2eb`. See `REPORT.md` for controls, logs, and validation steps.

Test 5B passed the hardware checklist. Its approved SHA-256 is `fb46c7b29dc1c0d0fb3928c43e00a462131d1237acd62e8d3d2563b953d5b04d`.

Test 6 built and passed offline checks. It imports Horizon's `ticket_levels.rs`, uses the original region format reader/writer and palette, and caches the meshes of nine resident columns. Streamed edits use the independent `switch_streaming_v1` save. The previous world is preserved. The new NRO SHA-256 is `cbb60c20af878c499e05dd378afaa2179dd78ce2e23521569763b0f152ba20df`. See `REPORT.md` for the test plan and controls.

The supplied Test 6 hardware log confirms eviction saves, readback and clean exit. The user reported stutter, with 19 transitions taking 29–101 ms before meshing.

Test 6B is confirmed on hardware. `world_stream.rs` runs storage work on one worker and checks revisions before eviction. `lib.rs` budgets one column mesh per frame and logs frame timings. Host tests passed (14 passed, 1 ignored). Test 6B SHA-256 is `04f188fbdb39831bb0c040f1a6667c57af7f5351083d4d92a1b3d4b601f5cabc`. The user reported smooth crossings. The log confirms player restoration, one saved column read, 12 asynchronous transitions, and a clean exit. It records no edits or dirty writes in that session.

Test 6C launched and exited cleanly on hardware. Its automatic callback was unreachable and the user stopped the waiting capture script. The SD log includes `RESULT=0` and `RETURN 0`. The artifact SHA-256 is `70497726858b0af6993e610c4e7c39292db16cf84a5741abfadd43b379dc110d`. Keep its source and binaries for reference and use the SD logger for later milestones. Test 6B remains the stable client baseline.

Test 7 built with the original 26.3 resource bundle and `GeneratorWorker::generate_proto`. The PC probe matched the existing Java terrain oracle for seed 12345, chunk (-2, 3), and confirmed 24 sections across Y −64 to 319, all six heightmaps, determinism and NBT round trip. Cold generation took 1,439 ms on WSL, with 1,425 ms in the structure stage. The warm repeat took 15 ms. The installed SDK lacks `pread`; Horizon OS now uses the existing locked seek/read path. Five prepared-file tests passed on WSL.

Test 7 was confirmed on physical Switch hardware (init 59,761 ms, first column 1,180 ms, repeat 64 ms, RESULT=0, RETURN 0). Hash SHA-256 is `38857a3a7648d7be6a5dbaaeae6284f67a2081f5e078af0f9f1af3785dbfd729`.

Test 8 integrates the full vanilla 26.3 generator (seed 12345, Y -64..319, 24 sections) with the asynchronous chunk streaming client (`StreamWorker`), section-based mesh budgeting (16-block vertical sections with frustum and occluding face culling), and an experimental world save at `sdmc:/switch/horizon/saves/switch_vanilla_test8/`.
- Offline validation passed: 11 vanilla streaming host tests passed (verifying 3x3 generation, Java oracle state/biome hash match, block edits, dirty section flags, and persistence). 14 legacy streaming tests passed (1 ignored).
- AArch64 ELF linked with 0 undefined symbols. Valid NRO0 header. RomFS packaged with 1,669 files (44 MiB).
- NRO artifact ready for physical hardware validation: `ferramentas/switch-tests/out/test8_vanilla_streaming.nro` (57,885,357 bytes, SHA-256 `106db3017167533a076c0c781f98cafa809767cea44ed8e9ab4876eef9f3fc72`).
- Deployment: `nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8_vanilla_streaming.nro`. Log written to `sdmc:/switch/horizon-tests/test8_vanilla_streaming.log`.
- Status: Awaiting physical validation on Nintendo Switch. Performance follow-up: investigate the ~60s cold initialization time observed during table loading and deserialization.
