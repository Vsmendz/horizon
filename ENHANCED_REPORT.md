# Enhanced verification

The attached session identifies Horizon 0.1.0 with NVK Mesa 26.2.4, git-7789c1de1d. It records the first loading frame at 13,936 ms, the UI bundle at 42,842 ms, core at 78,393 ms, world renderer at 80,850 ms and the tail at 106,924 ms. It shows 58.0 FPS at 20 seconds after world presentation and 36.2 FPS cumulatively at 640 seconds. The final log line is a save and pause request. It does not show a crash or the exit result. No additional relevant diagnostic ZIP was found in the checked Downloads paths.

The swapchain measures time blocked on the fence associated with reuse of the previous acquisition semaphore. Its image timer separately measures the call to acquire the next image. The reported 30.8 ms is an average of the former over 244 frames, with 0.7 ms for image acquisition. It is a CPU-side wait, not a timestamp query for GPU execution. The first-world-frame sample aggregates work since counters began and is not representative of one frame. Meshing counts reach 46,473 in a ten-second window. The log does not identify distinct section keys or job causes. The shader parser rejects packs declaring lights, so those packs require compatible renderer support as well as a schema update.

The Enhanced build skips the provisional full Vulkan initialization when the native framebuffer opens. It throttles splash drawing to 10 Hz. The splash uses the worker's current resource stage and an indeterminate segment. The first native frame stamp means queued to the compositor. The display can show it later, and the loader controls the time before `userAppInit`. The native buffers are closed before Vulkan surface creation, so a gap during that handoff still needs measurement. This build does not claim a reduction in SD loading time.

Host tests check splash pixels and row stride, movement of the progress segment, touch coordinate scaling, contact changes and event order. The touchscreen adapter routes a primary finger as a mouse pointer. Touch scrolling and multi-finger gestures are pending.

## Reproduction

Use `tools/switch-enhanced/prepare.py SOURCE COMPAT BEVY BUILD` in WSL. SOURCE is this branch, COMPAT is the local Test 12 compatibility source (audited libc, winit, platform and network adapters), and BEVY is John Uzoka's bevy-horizon at `3eac779b232ee47cd0eb957676c64386ac7b0f1d`. The source tree is archived from Enhanced HEAD. The compatibility overlay supplies build adapters only. The original client and wgpu-hal come from the v0.1.0 history. The static NVK archive was built from Mesa Switch base `606441c90e459028a50fca352daebfc20211f890` with the pinned bevy-horizon series. Its hash is `b4c4c498354356b50ecefc72cc73bfc778337ead7153a201cf5405ef05ef65df`. This is a rebuilt driver with different provenance from the release binary. Dependencies keep their upstream licenses in their own checkouts.

With devkitPro on PATH and `HORIZON_NX_NVK_LIB` pointing to the directory containing the localized driver:

```sh
cd "$build/source"
export CARGO_TARGET_DIR=/tmp/horizon-switch-target
export RUSTFLAGS='--cfg getrandom_backend="custom"'
cargo --config ../switch.toml update -p libc@0.2.190 --precise 0.2.191
cargo --config ../switch.toml rustc --locked --profile fast -p horizon-client \
  --bin horizon-client --features horizon-nx/vulkan --target ../aarch64-nintendo-horizon.json \
  -Zjson-target-spec -Zbuild-std=core,alloc,std,panic_abort \
  -Zbuild-std-features=compiler-builtins-mem -- -C link-arg=--defsym=horizon_switch_sysconf=sysconf
bash "$source/tools/switch-enhanced/package.sh" \
  "$CARGO_TARGET_DIR/aarch64-nintendo-horizon/fast/horizon-client" "$source/dist" \
  "$HORIZON_NX_NVK_LIB/libvulkan_bh.a"
```

## Console test

Copy the prepared 26.3 resource directory contents to `sdmc:/switch/horizon-enhanced/resources/`. Start the Enhanced NRO through an application title override. Record video from launch to title, test touch and controller input, create an experimental world, edit a block, exit and reopen. Collect `sdmc:/switch/horizon-enhanced/log.txt`. First test without a shader pack. Use the existing original NRO in `sdmc:/switch/horizon/` for rollback.
