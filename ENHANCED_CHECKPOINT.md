# Horizon NX Enhanced checkpoint

Branch `enhanced/horizon-nx` starts directly at John Uzoka's v0.1.0 release commit `9ee0e68f703c53861dc4891f4d4976c6dd34dd05`. The annotated tag is `4c7695cfe5390029742ef0944f3729ebf02bf4d3`. Both public `switch-release` refs agree with the peeled tag. The reference's MIT license and history are retained.

The release client now opens a libnx linear framebuffer before the normal service and resource work in main. The resource worker updates the real stage text while the render thread animates the indeterminate bar. UI readiness releases the framebuffer before Vulkan takes over the default NWindow. The native path skips the provisional full Vulkan renderer. The ordinary release renderer is still used for the title and world.

The Enhanced app uses `sdmc:/switch/horizon-enhanced/` for resources, settings, saves and logs, and its update checker and downloader are disabled. The log stamps start at `userAppInit`. Touch feeds the original GUI pointer path for presses, moves and releases.

Use [ENHANCED_REPORT.md](ENHANCED_REPORT.md) for measurements and the build and console procedure.
