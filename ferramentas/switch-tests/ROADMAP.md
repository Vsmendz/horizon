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
| Vanilla first-person polish | Awaiting physical validation | Test 9 integrates block display transforms, tick-based swing and equip animations, FOV, save recovery, water movement and underwater fog |
| Singleplayer mechanics and essential interfaces | Pending | Extend the validated world and connect the original player, inventory, crafting and menu systems |
| Entities, audio and performance tuning | Pending | Establish resource budgets from hardware measurements |

Each new hardware milestone uses a separate NRO. Experimental save formats use separate SD directories.

Test 6 measured a 41 ms median synchronous transition before meshing. Test 6B moved generation and region I/O to one worker. The current client streams the vanilla Overworld at Y −64 through 319 and retains the incremental per-section GPU upload budget.
