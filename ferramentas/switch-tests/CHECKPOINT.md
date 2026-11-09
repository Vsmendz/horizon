# Current Switch runtime checkpoint

## Test 10B continuation

The Test 10 hardware run (`test10_vanilla_world_visuals.log`) is confirmed as executed on physical hardware: 5,640 frames (58.57 FPS), 46 frame intervals above 33 ms, maximum interval 108.660 ms, first frame at 55.033 s, 2,011 emitted particles (peak 138), 23 breaks, 10 placements, two saved columns, `RESULT=0` and `RETURN 0`. The owner reported a visual defect on some woodland trees near spawn.

`test10b_woodland_models` fixes the Dark Forest woodland models (`c3836a45d`): giant mushrooms (`brown_mushroom_block`, `red_mushroom_block`, `mushroom_stem`) were missing from `VISUAL_BLOCKS` and fell back to `VoxelBlock::Stone`, and rotated log axes plus multipart mushroom faces needed per-`BlockStateId` model baking (`state_models.rs`, 192+ state models). It also integrates `crates/horizon-platform` (`available_space` via `libc::statvfs` on `target_os = "horizon"`, verified by 17 new compile-time ABI assertions in `abi_assertions.rs` for 180 SDK checks total, logged via `PLATFORM_STORAGE available_bytes=...`). Together with `2432d280a` (`arboard` and direct `rustix` isolation), `winit 0.30.13` is now the only remaining transitive path pulling `rustix 1.1.5` into `horizon-client`.

Test 10B (`out/test10b_woodland_models.nro`, 58,713,241 bytes, SHA-256 `237eb76dd56fbadba03858918702708e63db4bf1bde1955f2b8c277858604bb1`, ELF SHA-256 `7ded60dc41cd45936b876f7b71079fc74b16d09cba4c2c363f53bd7cf133e223`) ran on physical Switch hardware (`test10b_woodland_models.log`, `user_confirmed_execution`): 4,574 frames (~59.48 FPS), 7 intervals over 33 ms, maximum interval 38,995 µs, first frame at 62,666 ms, 180 SDK ABI assertions passed, `PLATFORM_STORAGE available_bytes=26329186304`, `FEATURES unsupported=0 failed=0`, two block placements, `RESULT=0` and `RETURN 0`. Visual confirmation of the woodland models and hardware particle emission (`EFFECTS emitted=0` because no blocks were broken in this run) remain pending; `MODEL_FALLBACK stone_names={...}` exposed the remaining full-block `Stone` fallbacks across the 35,723 registry states. All 21 earlier NRO digests match (`out/test10b_woodland_models.offline.log`), and all 1,843 RomFS files match staging. The host suite passed 35 tests (`out/test10b.gles.log`), legacy streaming passed 14 (`out/test10b.legacy.log`), and `horizon-platform` passed host unit tests and clippy (`out/platform.host.log`, `out/platform.clippy.log`).

Reproduce in WSL from the repository root:

```sh
source ferramentas/env.sh
CARGO_NET_OFFLINE=true python3 ferramentas/switch-tests/build.py test10b_woodland_models
python3 ferramentas/switch-tests/verify_artifact.py test10b_woodland_models ferramentas/switch-tests/out/test10-romfs
```

Test 10B stages only private local game resources in the ignored output directory. It needs the Test 7 worldgen bundle and the Java 26.3 gameplay facts. The report records how to export them. Structures, full light propagation and full-game integration remain separate work.

For the next session, use the matrix in `ROADMAP.md`. After `arboard`, direct `rustix` and `fs4` (`horizon-platform`) isolation, `winit 0.30.13` is the sole remaining transitive `rustix` dependency in `horizon-client`. Keep the working GLES input/render loop and isolate authoritative server initialization and ticks from `winit` and desktop UI/audio dependencies. The original item NBT and in-memory protocol pipe (`horizon_protocol::connection::Pipe`, already linked as ELF64 AArch64 in the scratch harness) are the next integration points.

## Historical checkpoints

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

The user supplied the Test 8 hardware log and photograph in the project root. The photograph shows blue sky, a dark crosshair and a black terrain-shaped region. The log confirms 10,982 presented frames, chunk crossings, 14 breaks, six placements, two saved columns, and a clean exit. It does not establish readback after reopening. The 6 MiB upload cap excluded sections on 111 frames. Initial block tables took 55,716 ms, with process allocator usage of 188,782,624 bytes after the first frame. Test 8 is not approved. The terrain shader clamps its light to at least 0.25, but this does not identify the source of black pixels. Investigate atlas upload, framebuffer output, vertex attributes and overlay with an offscreen pixel test. The current objective is Test 8B; preserve Test 8 and prior NROs.

The existing host render smoke test can now build the Switch test crate on Linux by enabling the native EGL dependencies and excluding the Horizon-only entry point from the host build. It passed for the prepared 26.3 data. The generated spawn was (103.5, 136.05, -27.5), 83 sections had nonempty meshes, and the complete nine-column mesh contained 148,386 vertices, or 7,716,072 bytes. The renderer's frame upload limit is 6 MiB and it selects a subset of visible sections. This shows a possible source of missing geometry, but does not establish the cause of black pixels. Output is in `out/test8_vanilla_streaming.render-host.log`. Further work needs a pixel-level EGL readback and/or the missing hardware log.
- Offline validation passed: 11 vanilla streaming host tests passed (verifying 3x3 generation, Java oracle state/biome hash match, block edits, dirty section flags, and persistence). 14 legacy streaming tests passed (1 ignored).
- AArch64 ELF linked with 0 undefined symbols. Valid NRO0 header. RomFS packaged with 1,669 files (44 MiB).
- NRO artifact ready for physical hardware validation: `ferramentas/switch-tests/out/test8_vanilla_streaming.nro` (57,885,357 bytes, SHA-256 `106db3017167533a076c0c781f98cafa809767cea44ed8e9ab4876eef9f3fc72`).
- Deployment: `nxlink -a <SWITCH_IP> ferramentas/switch-tests/out/test8_vanilla_streaming.nro`. Log written to `sdmc:/switch/horizon-tests/test8_vanilla_streaming.log`.
- Status: Awaiting physical validation on Nintendo Switch. Performance follow-up: investigate the ~60s cold initialization time observed during table loading and deserialization.

Test 8B is ready for physical diagnostics. The Linux GLES test passed with the real WGSL, a synthetic 42-layer atlas and a separate overlay draw. The black pixels were not reproduced. The NRO records readback for the actual atlas and swapchain on Switch. It also uploads terrain sections incrementally to resident buffers, using the 6 MiB budget for staging. The next hardware run should capture the SD log and confirm the terrain and hotbar visually.

- Artifact `out/test8b_vanilla_render_fix.nro`, SHA-256 `628a399b73905d7c4b70f82f3b2ada639fb519d17be9c4fe08f68ad65a6af540`. All 14 previous NRO hashes verified. The 8B RomFS matches Test 8 byte for byte.
- Host tests are in `out/test8b.gles.log` (24 passed, two ignored) and `out/test8b.legacy.log` (14 passed, one ignored). Detailed WSL timing data is in `out/test8b.block-tables.log`. The console needs its own substage timings before changing the loader.
- Run the uploader without its log callback and retrieve `sdmc:/switch/horizon-tests/test8b_vanilla_render_fix.log`. See `REPORT.md` for commands and diagnostics.

The 8B hardware run still shows black terrain and a missing hotbar. All 42 atlas layers matched; its three opaque-black samples lacked a known sky control. The production WGSL matches the 6B, 8 and 8B NROs byte for byte. The Test 8 change in atlas size, mesh construction and staging is documented in `REPORT.md`.

8C adds a known blue clear frame and a five-panel frame with solid, sampled, lit, HUD and direct texel draws. It logs samples after the world pass, after the HUD pass and after the window blit. The shared readback helper and the panel colors passed in Linux GLES. The next physical run uses `test8c_gles_pipeline_probe.nro` and saves `sdmc:/switch/horizon-tests/test8c_gles_pipeline_probe.log`. The cause on Switch remains open pending these samples.

8C passed the AArch64 and NRO checks. Its RomFS matches Test 8, and all 15 earlier NRO hashes match. SHA-256 `10a81bdcc68ded6db72f39cacdeeee99992befb60a07d3ff2147e5bbadc4ca47`.

The 8C hardware log confirms the intermediate readback. The known blue clear, green solid geometry and purple solid HUD matched. Both filtered texture panels and the direct texel panel failed despite the 42 exact atlas attachment readbacks. The EGL window size query returned zero, so no window readback was captured. The failure is already present before the window blit.

8D adds a fixed layer-0 texel panel and records the sampler uniform, unit, texture and sampler objects, mip range, and view range used by the HAL. After the first normal frame it tests the same atlas and isolated GLES controls with a plain 2D texture and 15- and 42-layer arrays. See `REPORT.md` for the interpretation and hardware procedure. The cause is pending the 8D console log.

8D passed the Linux GLES and vanilla suite (24 passed, two ignored), legacy streaming (14 passed, one ignored), and static AArch64 and NRO checks. The RomFS matches Test 8 byte for byte, and all 16 earlier NRO hashes match. Artifact `out/test8d_gles_atlas_probe.nro`, SHA-256 `f51d9cbb672a0622821c4ecdc9b77bb6dac4310e25c4927da02e011680b203fc`.

The 8D console log identified the regression. The 42-layer atlas was allocated as `GL_TEXTURE_CUBE_MAP_ARRAY` (`0x9009`), while the shader used `sampler2DArray` on unit 0. The HAL heuristic chooses cube storage for square, single-sample textures with a multiple of six layers. The 6B atlas had 15 layers and used the 2D array path. GLES controls passed at 15 and 42 layers on NV120. The original diagnostic attempted a second target for the same object, which explains its `GL_INVALID_OPERATION`.

8E explicitly allocates the production atlas as a 2D array and adds a separate 42-layer texture control through the same HAL upload, bind-group layout and sample shader. Native sampling checks the target before binding and leaves the atlas storage and parameters intact. The expected control pixel is `[92, 212, 56, 255]`. See `REPORT.md` for the physical validation procedure.

8E passed the Linux GLES and vanilla suite (24 passed, two ignored), legacy streaming (14 passed, one ignored), and static AArch64 and NRO checks. The production WGSL matches 8D byte for byte, the RomFS matches Test 8, and all 17 earlier NRO hashes match. Artifact `out/test8e_gles_atlas_fix.nro`, SHA-256 `78554fb987fd8d81116d0c0c69bfe2d4d6ab1be999c3629dc90560bb1dc338c7`.

8E is approved on physical Switch. The supplied log confirms the 42-layer GLES 2D array, exact framebuffer control colors, asynchronous streaming, 5,282 frames, eight breaks, eleven placements, three saved chunks, `RESULT=0` and `RETURN 0`. The user confirmed textured terrain and hotbar. The 8E artifact is preserved and its recorded hash matches. Water still rendered as opaque blue.

8F reuses the original Horizon fluid tessellator and registry states for water. It adds a GLES alpha-blended terrain pass with the original still, flow and overlay sprites. It keeps the 8E save path and format. See `REPORT.md` for tests, current approximations and the physical checklist.

8F passed the Linux GLES and vanilla suite (26 passed), legacy streaming (14 passed), and the regular desktop mesher suite (55 passed). The static AArch64 ELF, NRO and all 1,672 RomFS files passed verification, and the 18 earlier NRO hashes match. Artifact `out/test8f_vanilla_water_fix.nro`, SHA-256 `e91c33a629e3f14f26e4219ace680636f66447b44e422916e6cf73b6ea92d6e6`. Physical validation is pending.

The 8F hardware log confirms execution and clean exit, with 6,851 frames, six breaks, 43 placements, one saved chunk, `RESULT=0` and `RETURN 0`. Water appearance outside the submerged region remains pending. The 19 prior NRO hashes matched before Test 9. The local recovery branch `switch-test9-checkpoint` points to `6d000a895`.

Test 9 shares the desktop hand motion and equip state, uses baked model display transforms, adds configurable FOV and preserves immediate stick look. It checks the actual fluid and collision at a restored pose, supports upward and downward water movement and blends biome fog when the camera submerges. See `REPORT.md` for the hardware checklist and remaining parity work.

Test 9 passed static AArch64 and NRO checks. All 1,672 RomFS files match the 8F image and all 19 earlier NRO hashes match. Artifact `out/test9_vanilla_first_person_polish.nro`, SHA-256 `cdcfa680ce8b76710655762d0e6a2a337727d59a3b41120797210a157104a9cb`. Awaiting hardware validation.

Test 9 was visually approved on physical Switch (`test9_vanilla_first_person_polish.log`: 33,252 frames at 59.66 FPS, 19 breaks, 113 placements, one saved column, `RESULT=0`, `RETURN 0`).

Test 10 (`out/test10_vanilla_world_visuals.nro`, SHA-256 `83ba0e8e2e920fac39ab71b0f388b756be74423dd7615c9067d9cbecce810643`) executed on physical Switch (`test10_vanilla_world_visuals.log`: 5,640 frames, 23 breaks, 10 placements, two saved columns, `RESULT=0`, `RETURN 0`; visual defect reported on some woodland trees).

Test 10B (`out/test10b_woodland_models.nro`, SHA-256 `237eb76dd56fbadba03858918702708e63db4bf1bde1955f2b8c277858604bb1`) executed on physical Switch (`test10b_woodland_models.log`: 4,574 frames at ~59.48 FPS, 7 intervals above 33 ms, max interval 38.995 ms, first frame 62,666 ms, 180 SDK ABI assertions, `PLATFORM_STORAGE available_bytes=26329186304`, `FEATURES unsupported=0 failed=0`, two placements, `RESULT=0`, `RETURN 0`). `MODEL_FALLBACK` in `test10b_woodland_models.log` revealed that Minecraft 26.3 woodland features place `poplar_log`, `orange_poplar_leaves`, `red_poplar_leaves`, `yellow_poplar_leaves`, `pale_oak_log`, `pale_oak_leaves`, `azalea_leaves`, `flowering_azalea_leaves`, and `bee_nest` (plus hundreds of other vanilla blocks) that were outside the manual 68-block `VoxelBlock` list and fell back to `Stone` or `Air`.

Test 11 (`out/test11_vanilla_model_catalog.nro`, 61,974,429 bytes, SHA-256 `34b76d6a587bd2b31874237208ad0149f9dc03df5be1eef60e6de8dab81700c5`) replaces the manual block-model whitelist with `StateModelCatalog` in `horizon-client-switch/src/state_models.rs`, binding all 1,286 registered blocks across all 35,723 `BlockStateId`s (`MODEL_COVERAGE total_states=35723 bound_states=35723 model_states=35115 entity_proxy_states=460 fluid_states=34 invisible_states=114 unique_blocks=1286 unique_variants=7883 baked_quads=80079 missing_states=0 stone_fallbacks=0`). `prepare_switch_assets.py --vanilla-all-models` packs all 1,288 blockstates, 2,794 transitive models, and 1,308 transitive textures into three sequential bundle files (`assets/blockstates_bundle.json`, `assets/models_bundle.json`, `assets/textures_bundle.bin`, 3.18 MiB total across 6 asset files in `out/test11-romfs`). Host GLES/vanilla tests (36 passed in `out/test11.gles.log`), legacy streaming tests (14 passed in `out/test11.legacy.log`), and static AArch64/NRO/RomFS verification (`out/test11_vanilla_model_catalog.offline.log`, all 22 prior NRO hashes preserved) passed. Awaiting physical Switch validation.

Test 12 (`out/test12_client_crates.nro`, 63,100,829 bytes, SHA-256 `6a52b4f2feca22fe279ba5102b5ebc87ec88c80bdf3c31ae1eb1eb83924365b1`, ELF SHA-256 `2a87daed4077bef4297ab49a39f9cb46bc9be89bdcad5af087b4b8d6db5fc81c`) unblocks and links all 27 original Horizon crates (`horizon-core`, `horizon-platform`, `horizon-threads`, `horizon-world`, `horizon-client-core`, `horizon-client-input`, `horizon-command`, `horizon-protocol`, `horizon-items`, `horizon-assets`, `horizon-theme`, `horizon-cosmetics`, `horizon-hdr`, `horizon-narrator`, `horizon-optifine`, `horizon-shaders`, `horizon-shader-api`, `horizon-telemetry`, `horizon-bench`, `horizon-server-files`, `horizon-anticheat`, `horizon-chat-filter`, `horizon-mcp`, `horizon-friends`, `horizon-mods`, `horizon-client-render`, `horizon-client-lib`, and `horizon-client`) on `aarch64-nintendo-switch-newlib`. Desktop-only dependencies (`rustix`, `wayland-client`, `x11-dl`, `calloop` in `winit 0.30.13` via `ferramentas/switch-tests/vendor/winit/src/platform_impl/horizon`; `mimalloc` in `horizon-threads`; `ash` and `wgpu/vulkan` in `horizon-hdr`, `horizon-client-render`, `horizon-client-lib` and `horizon-client`; `ureq/tls` and `ring` in `horizon-cosmetics`, `horizon-telemetry`, `horizon-server-files`, `horizon-client-render`, `horizon-client-lib` and `horizon-client`; `socket2` and `if-addrs` in `horizon-friends`; `mio` in `horizon-client-lib`/`lan::io_pool`) are isolated with `cfg(not(target_os = "horizon"))`, and `crates/horizon-platform/src/newlib.rs` provides both `__getrandom_v03_custom` (`getrandom` 0.3/0.4) and `__getrandom_custom` (`getrandom` 0.2) backed by `libc::getrandom` -> libnx `randomGet`. `horizon-client-switch/src/client_bridge.rs` links and exercises `horizon-threads`, `horizon-client-input` (Nintendo Switch `GlyphStyle::Nintendo`, `Button::by_letter`, `Edges`, `move_vector`, `look_delta`, `KeyBinds`), `horizon-command` (2,446-node Brigadier vanilla command tree and `network::from_packet`), `horizon-protocol` (`Connection::local` over `Pipe::pair` with zlib threshold compression), `horizon-client-render` (`camera` and `OptionsFile`), `horizon-client-lib` (`ticket_levels` and `save::VERSION_NAME`), and `horizon-client` (`game::exit_code`, `mod_api`), logging `CLIENT_CRATES status=ok crates=27 ...` at startup. All 23 prior NRO hashes and 1,550 RomFS files verified (`out/test12_client_crates.offline.log`). Awaiting physical Switch validation.
