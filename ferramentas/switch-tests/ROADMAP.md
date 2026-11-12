# Switch port roadmap

| Milestone | Status | Gate |
| --- | --- | --- |
| Runtime, EGL/GLES and wgpu-hal | Hardware confirmed | Tests 1–4 |
| First-person prototype with edits, save, held item and logs | Hardware confirmed | Test 5B |
| Ticket-driven column streaming and multi-region saves | Hardware confirmed with stutter | Test 6 console log shows eviction, readback and clean shutdown |
| Asynchronous transitions and per-frame mesh budget | Hardware confirmed | Test 6B. User reported smooth crossings and the console log shows 53–75 µs to apply each result |
| Automatic nxlink log capture with SD fallback | Deferred | Test 6C ran on hardware. Callback unreachable. Use SD logs for subsequent tests |
| Vanilla generator resource audit | Completed offline | The existing 51-file RomFS contains render assets. The generator additionally requires the matching block registry and worldgen bundle, which this checkout lacks |
| Isolated vanilla terrain | Hardware confirmed | Test 7. Matched Java 26.3 reference, heightmaps, NBT and determinism on physical Switch |
| Vanilla terrain in the playable client | Hardware confirmed | Test 8E confirmed textured terrain, the hotbar, asynchronous streaming, edits and clean shutdown |
| Vanilla water geometry and transparency | Executed on hardware | Test 8F exited cleanly. Confirm the shoreline and water appearance outside the submerged region |
| Vanilla first-person polish | Hardware confirmed | Test 9 verified water and first-person animation. Held water was oversized |
| Vanilla decoration and visual effects | Executed on hardware | Test 10 rendered decorated streaming, particles and bucket model (first frame 55.033 s, 5,640 frames); owner reported a visual defect on some woodland trees |
| Woodland state models and platform storage | Executed on hardware | Test 10B (`test10b_woodland_models.log`): 4,574 frames (~59.48 FPS), max interval 38.995 ms, 180 ABI assertions, `PLATFORM_STORAGE available_bytes=26329186304`, `FEATURES unsupported=0 failed=0`, `RESULT=0`. Visual tree appearance pending; `MODEL_FALLBACK` identified remaining full-block `Stone` fallbacks |
| Complete 35,723-state vanilla model catalog | Compiled and verified offline; awaiting hardware | Test 11 (`test11_vanilla_model_catalog.nro`, SHA-256 `34b76d6a587bd2b31874237208ad0149f9dc03df5be1eef60e6de8dab81700c5`): binds all 1,286 blocks / 35,723 states (`missing_states=0`, `stone_fallbacks=0`), 1,308 textures in a 3.18 MiB bundle |
| Full 27-crate original Horizon client graph on Switch | Compiled and verified offline; awaiting hardware | Test 12 (`test12_client_crates.nro`, SHA-256 `6a52b4f2feca22fe279ba5102b5ebc87ec88c80bdf3c31ae1eb1eb83924365b1`): links `horizon-client`, `horizon-client-lib`, `horizon-client-render`, `horizon-client-input`, `horizon-command`, `horizon-protocol`, and `horizon-threads` (`CLIENT_CRATES status=ok crates=27`) |
| Original Horizon-NX client and NVK | ELF/NRO verified offline | `horizon_switch_hybrid.nro` links the original client and NVK. Verify the menu, integrated world, inventory, audio and save on Switch |
| Singleplayer mechanics and essential interfaces | Pending | Extend the validated world and connect the original player, inventory, crafting and menu systems |
| Entities, audio and performance tuning | Pending | Establish resource budgets from hardware measurements |

Each new hardware milestone uses a separate NRO. Experimental save formats use separate SD directories.

Test 6 measured a 41 ms median synchronous transition before meshing. Test 6B moved generation and region I/O to one worker. The current client streams the vanilla Overworld at Y −64 through 319 and retains the incremental per-section GPU upload budget.


## Playable port matrix

| Subsystem | Current state | Original code and remaining integration |
| --- | --- | --- |
| Startup and shutdown | Test 9 hardware confirmed. Test 10 awaiting hardware | Custom Rust/newlib runtime, libnx and EGL. Validate first-frame time and clean exit with decoration |
| World, chunks and biomes | Partial | Original `GeneratorData`, `OverworldGenerator`, terrain, carvers and FEATURES. Compare decoration against Java 26.3 and validate tree boundaries |
| Structures and dimensions | Available in Horizon | Original structure generation and level management. Connect persistence for block entities, entities and per-dimension storage before enabling |
| Streaming and storage | Test 9 hardware confirmed. Test 10 awaiting hardware | Original `RegionFile`, NBT palettes and heightmaps. Separate Test 10 world. Player persistence currently covers position, rotation and selected slot |
| Controls, physics and animation | Partial | Shared movement, collision, raycasting, camera and first-person animations, with libnx input. Full controller UI navigation and rebinding pending |
| Textures, fluid geometry and HUD | Test 9 hardware confirmed. Test 10 awaiting hardware | Existing GLES atlas and water pass. Test 10 adds cutouts, biome tints, the held bucket and particles. HUD currently covers the crosshair and hotbar |
| Lighting, sky and weather | Partial | Test 10 adds local emission and biome sky color. Bring over the original light engine, the world clock, sky passes and weather once a shared simulation drives them |
| Blocks, items and inventory | Partial | Original block registry, collision, `PlayerInventory` and item baking. The hotbar still holds prototype block stacks. Persist actual item stacks and equipment before crafting and containers |
| Mining, crafting and survival | Partial | Test 10 adds held-trigger mining with exported hardness. The original recipe matching, `FoodData`, damage and death mechanics exist. Connect them to the authoritative singleplayer simulation |
| Fluids and redstone simulation | Available in Horizon | Fluid shape and tint render on Switch. Original level ticks and fluid/redstone behaviors still need an authoritative tick loop |
| Entities, AI and spawning | Available in Horizon | Original mobs, entity tracking, spawn/despawn and NBT. Need level tick integration, rendering and entity persistence |
| Menus, world selection and settings | Platform adaptation | Desktop winit UI needs a Switch event and input boundary. Existing text and layout logic should remain shared |
| Audio | Platform adaptation | Desktop mixer and sound events exist. A libnx PCM output path, asset preparation and audio latency tests are pending |
| Networking and commands | Platform adaptation | Reuse the original memory pipe before exposing LAN. Desktop integrated server is still coupled to `Game`, `Audio` and UI resources. Switch socket/runtime and crypto paths require separate validation |

The next integration boundary is the authoritative server. Start with the in-memory `horizon_protocol::connection::Pipe` used by the desktop integrated server and move the required simulation inputs out of the winit-owned `Game`. Keep the Switch window, GLES and pad code at the client boundary. Persist original item stacks and level data there before adding crafting and entities. The stable `horizon_switch.nro` name is reserved for a later consolidated build.
