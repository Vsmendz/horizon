# Original client integration on Switch

The integration builds the Horizon-NX release source in a separate WSL tree. The current Horizon workspace provides the measured newlib bindings, the storage and RNG adapters, and the winit base. The original runtime, controller backend, software keyboard, menu, renderer, integrated server, world simulation and save code come from Horizon-NX. Its own wgpu-hal Vulkan implementation presents through `VK_NN_vi_surface`; the overlay connects wgpu 30.0.1 and libnx's real `NWindow` to that implementation.

| Source | Pinned revision | Role |
| --- | --- | --- |
| [Horizon fork](https://github.com/Vsmendz/horizon/commit/6637dfe553db818914687e1c292ad0f10456d33d) | `6637dfe553db818914687e1c292ad0f10456d33d` | Test 12 and preserved GLES baseline |
| [Horizon-NX v0.1.0](https://github.com/JohnUzoka/horizon-nx/releases/tag/v0.1.0) | `9ee0e68f703c53861dc4891f4d4976c6dd34dd05` | Full client, libnx lifecycle and Vulkan HAL |
| [bevy-horizon](https://github.com/JohnUzoka/bevy-horizon/tree/3eac779b232ee47cd0eb957676c64386ac7b0f1d) | `3eac779b232ee47cd0eb957676c64386ac7b0f1d` | Horizon window handles, audio, build target and NVK patches |
| [Mesa Switch fork](https://github.com/JohnUzoka/mesa-switch/tree/606441c90e459028a50fca352daebfc20211f890) | `606441c90e459028a50fca352daebfc20211f890` | Compatible base for the bevy-horizon NVK patch series |

The annotated release tag peels to the same commit as `switch-release` in both public repositories. The observed `switch` tip was `d9de2d37088f73a27f2ff686925386ac4596db23`, a separate revision after an upstream merge. The common ancestor of the release and our baseline is `d5c4248f628399723eb3adbd61a29187806259ae`. The release's initial Switch port commit is `dd018e048`. Its later commits include changes to world generation as well as platform work. These builds therefore use the release's original simulation and streaming together.

John Uzoka and the Horizon-NX contributors authored the reference client and runtime. The bevy-horizon build and localization approach is credited to John Uzoka and its [listed contributors](https://github.com/JohnUzoka/bevy-horizon/blob/3eac779b232ee47cd0eb957676c64386ac7b0f1d/CREDITS.md). Mesa Switch is by danfromtico and contributors including NaGaa95. Horizon-NX keeps Horizon's MIT license. The bevy-horizon tooling is MIT or Apache-2.0 and each upstream patch dependency retains its own notices. The scripts use the original source and license files in the external checkouts. They leave those dependencies and all prepared game files outside Git.

## Build

Run in WSL. Source `ferramentas/env.sh` from the original checkout with devkitPro. Keep the three reference checkouts outside both worktrees and use the exact revisions above. Use a Linux filesystem for their builds.

```sh
source "/mnt/c/Users/Pc vinw/Documents/horizon/ferramentas/env.sh"
integration="/mnt/c/Users/Pc vinw/Documents/horizon-nx-integration/ferramentas/switch-tests/hybrid"
history="/mnt/c/Users/Pc vinw/Documents/horizon"
python3 "$integration/prepare.py" reference bevy-reference build
(cd build/source && cargo --config ../switch.toml update -p libc@0.2.190 --precise 0.2.191)
bash "$integration/build-nvk.sh" mesa-switch bevy-reference tools
bash "$integration/build-client.sh" build bevy-reference/nvk/lib "$history"
```

The native driver build uses Meson 1.9.2, Ninja, GCC/G++, LLVM/Clang 15 with libclc and LLVM SPIR-V translator, Mako, PyYAML, pkg-config, bison and flex. The host `tools` prefix contains SPIRV-Tools and SPIRV-Headers at `vulkan-sdk-1.3.290.0`. Build and install SPIRV-Tools there with CMake/Ninja and `SPIRV_SKIP_TESTS=ON`. Rust nightly needs rust-src, the `aarch64-unknown-linux-gnu` standard library for Mesa's NAK/NIL archives, and bindgen-cli and cbindgen. The app uses the separate Horizon/newlib target and libnx. Localization binds Mesa's Rust symbols within the driver object before it is linked into the app. The build requires `aarch64-linux-gnu-gcc` for Meson's Rust compiler check and devkitA64 for the final client.

The verified SDK is also available locally under `~/.cache/horizon-nx-integration/sdk-recovery/ferramentas/devkitpro`. If the original checkout is sparse, export `DEVKITPRO` to that directory, `DEVKITA64="$DEVKITPRO/devkitA64"`, and add `$DEVKITA64/bin`, `$DEVKITPRO/tools/bin` and `$HOME/.cargo/bin` to `PATH`. On Ubuntu 24.04 the Clang 15 OpenCL headers are in `/usr/lib/llvm-15/lib/clang/15.0.7/include`. `mesa_clc` linked to LLVM 15 expects them under `clang/15/include`. This build used a `clang/15` symlink to `15.0.7`.

The app build writes its verified ELF and NRO under this worktree's `ferramentas/switch-tests/out/`. It checks prior NRO hashes from the original checkout. `ALLOW_MISSING_HISTORY=1` explicitly permits the audit to proceed with a list of missing files if the original checkout is sparse. A digest mismatch always fails. The final link aliases the measured libc's `horizon_switch_sysconf` to the reference runtime's `sysconf`. Both use `_SC_PAGESIZE=8` from devkitA64.

The source overlay disables the reference updater and forces the app's writable root to `sdmc:/switch/horizon-hybrid`. The experimental client starts at its menu by default. The app uses the system allocator and system clocks. Device presence, performance and clean shutdown remain console checks.

## Hardware validation

Use an application title override in Sphaira. Supply the legally prepared 26.3 directory containing `horizon-resources.json` at `sdmc:/switch/horizon-hybrid/resources/`. Install or send only the verified integration NRO. Start at the menu, create a new world, cross several chunk boundaries, open the inventory, craft or place a block, save and exit. Reopen and confirm the edit. Collect `sdmc:/switch/horizon-hybrid/log.txt` and report whether Sphaira returned normally. Test the standard renderer before shader packs. To roll back, launch a retained copy of an earlier NRO from the SD card with its original save directory.

The local bundle for this build is under `~/.cache/horizon-nx-integration/prepare-fresh/.local/horizon-client/26.3/`. It contains resources prepared from the owner's installed Minecraft. Copy the directory's contents onto the SD card. Install sound resources separately with `horizon-prepare sounds` to test audio.
