# Horizon NX Enhanced checkpoint

Branch `enhanced/horizon-nx` starts directly at John Uzoka's v0.1.0 release commit `9ee0e68f703c53861dc4891f4d4976c6dd34dd05`. The annotated tag is `4c7695cfe5390029742ef0944f3729ebf02bf4d3`. Both public `switch-release` refs agree with the peeled tag. The reference's MIT license and history are retained.

The release client now opens a libnx linear framebuffer before the normal service and resource work in main. The resource worker updates the real stage text while the render thread animates the indeterminate bar. UI readiness releases the framebuffer before Vulkan takes over the default NWindow. The native path skips the provisional full Vulkan renderer. The ordinary release renderer is still used for the title and world.

The Enhanced app uses `sdmc:/switch/horizon-enhanced/` for resources, settings, saves and logs, and its update checker and downloader are disabled. The log stamps start at `userAppInit`. Touch feeds the original GUI pointer path for presses, moves and releases.

Use [ENHANCED_REPORT.md](ENHANCED_REPORT.md) for measurements and the build and console procedure.

The offline-verified artifact is `dist/horizon_nx_enhanced.nro`, 51,313,553 bytes. SHA-256 `6d6777b765b3d94ced73205eccc00ee932a4d2ca14bdad4a220a07b599568432`. The linked ELF is 66,905,392 bytes with SHA-256 `a2c06d60a4d15d5ecade9ee63d7d80ea9da9e7fce6e2b0134bb0033bfc82177f`. Executable sources are from `ff0aff0c6` with the SDK overlay; subsequent commits add the tools and report. The package provenance records `d6191a5dc`. The archive has the original title and world code, the native splash and the HID adapter. Physical execution, cold and warm timings and save reopen are pending.
