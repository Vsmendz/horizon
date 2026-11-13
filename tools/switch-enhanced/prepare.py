"""Assemble the pinned original Horizon-NX client in an isolated WSL build tree."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

NX = "9ee0e68f703c53861dc4891f4d4976c6dd34dd05"
BEVY = "3eac779b232ee47cd0eb957676c64386ac7b0f1d"
WGPU = {
    "wgpu": "527ccdf43dd5b2e8676eed9984ce00e2bbb0a1b85b70c1969dcb6cd2eb55ab9e",
    "wgpu-core": "14c018fce9b6270aa203c2fdd56f3cce996713534bd757e4ea58c8560b121f14",
    "wgpu-core-deps-windows-linux-android": "7586165fd5f6d881cb9ce4bb71f40d6caab2c0f1837e3fc1d9788a197fb6004f",
}


def run(*args):
    return subprocess.check_output([str(arg) for arg in args], text=True).strip()


def replace(file, old, new):
    source = file.read_text()
    if source.count(old) != 1:
        raise ValueError(f"{file}: expected one occurrence of {old!r}")
    file.write_text(source.replace(old, new))


def patch_wgpu(patches):
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    result = {}
    for name, checksum in WGPU.items():
        filename = f"{name}-30.0.1.crate"
        archive = next(cargo_home.glob(f"registry/cache/*/{filename}"), patches / filename)
        if not archive.is_file():
            with urllib.request.urlopen(f"https://static.crates.io/crates/{name}/{filename}", timeout=60) as response:
                archive.write_bytes(response.read())
        if hashlib.sha256(archive.read_bytes()).hexdigest() != checksum:
            raise ValueError(f"Unexpected checksum for {filename}")
        with tarfile.open(archive) as tar:
            tar.extractall(patches, filter="data")
        result[name] = patches / f"{name}-30.0.1"
    cfg = 'target_os = "freebsd"'
    switch = cfg + ', target_os = "horizon"'
    replace(result["wgpu"] / "build.rs", cfg + '), feature = "vulkan")', switch + '), feature = "vulkan")')
    core = result["wgpu-core"]
    replace(core / "build.rs", 'all(windows_linux_android, feature = "vulkan")',
            'all(any(windows_linux_android, target_os = "horizon"), feature = "vulkan")')
    replace(core / "Cargo.toml", cfg + '))\'.dependencies.wgpu-core-deps-windows-linux-android]',
            switch + '))\'.dependencies.wgpu-core-deps-windows-linux-android]')
    replace(result["wgpu-core-deps-windows-linux-android"] / "Cargo.toml",
            'target_os = "netbsd"))\'.dependencies.wgpu-hal]',
            'target_os = "netbsd", target_os = "horizon"))\'.dependencies.wgpu-hal]')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("compat", type=Path, help="Test 12 checkout with audited newlib and winit")
    parser.add_argument("bevy", type=Path)
    parser.add_argument("build", type=Path, help="new scratch directory")
    args = parser.parse_args()
    repo, compat, bevy, build = (p.resolve() for p in (args.source, args.compat, args.bevy, args.build))
    assert run("git", "-C", repo, "rev-parse", "v0.1.0^{}") == NX
    assert run("git", "-C", bevy, "rev-parse", "HEAD") == BEVY
    assert not run("git", "-C", bevy, "status", "--porcelain", "--untracked-files=no")
    tests = compat / "ferramentas/switch-tests"
    sdk = Path(os.environ["DEVKITPRO"]).resolve()
    gcc = sdk / "devkitA64/bin/aarch64-none-elf-gcc"
    gcc_version = run(gcc, "-dumpfullversion")

    build.mkdir(parents=True)
    source = build / "source"
    source.mkdir()
    with subprocess.Popen(["git", "-C", str(repo), "archive", "HEAD"], stdout=subprocess.PIPE) as archive:
        subprocess.run(["tar", "-xf", "-", "-C", str(source)], stdin=archive.stdout, check=True)
        if archive.wait() != 0:
            raise RuntimeError("git archive failed")

    patches = build / "patches"
    patches.mkdir()
    # The original Bevy patches keep their own licenses and are read directly
    # from the pinned checkout. This workspace supplies the audited libc and winit.
    winit = patches / "winit"
    shutil.copytree(tests / "vendor/winit", winit)
    backend = winit / "src/platform_impl/horizon/mod.rs"
    replace(backend, "pub fn request_redraw(&self) {}", "pub fn request_redraw(&self) { REDRAW.store(true, Ordering::Release); }")
    old = backend.read_text()
    start = old.index("        let handle = rwh_06::OrbitalWindowHandle::new(")
    end = old.index("\n    }", start)
    backend.write_text(old[:start] + """        let window = std::ptr::NonNull::new(unsafe { nwindowGetDefault() })
            .ok_or(rwh_06::HandleError::Unavailable)?;
        Ok(rwh_06::HorizonWindowHandle::new(window).into())""" + old[end:])
    replace(backend, "use std::cell::Cell;", """use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static REDRAW: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn nwindowGetDefault() -> *mut std::ffi::c_void;
    fn appletGetFocusState() -> u32;
}""")
    backend.write_text(backend.read_text().replace(
        "rwh_06::OrbitalDisplayHandle::new()", "rwh_06::HorizonDisplayHandle::new()"
    ).replace("rwh_06::RawDisplayHandle::Orbital(", "rwh_06::RawDisplayHandle::Horizon("))
    replace(backend, """        while let Ok(user_event) = self.user_events_receiver.try_recv() {
            event_handler(event::Event::UserEvent(user_event), &self.window_target);
        }
        event_handler(event::Event::AboutToWait, &self.window_target);""", """        let mut background = false;
        let mut cause = StartCause::Poll;
        while !self.window_target.exiting() {
            event_handler(event::Event::NewEvents(cause), &self.window_target);
            // libnx AppletFocusState_Background. Horizon's ApplicationHandler
            // drains the message queue and manages saving and exit.
            let now_background = unsafe { appletGetFocusState() } == 3;
            if now_background != background {
                let event = if now_background { event::Event::Suspended } else { event::Event::Resumed };
                event_handler(event, &self.window_target);
                background = now_background;
            }
            for user_event in self.user_events_receiver.try_iter().take(64) {
                event_handler(event::Event::UserEvent(user_event), &self.window_target);
            }
            if !background && REDRAW.swap(false, Ordering::AcqRel) && !self.window_target.exiting() {
                event_handler(event::Event::WindowEvent {
                    window_id: window::WindowId(WindowId(1)),
                    event: event::WindowEvent::RedrawRequested,
                }, &self.window_target);
            }
            event_handler(event::Event::AboutToWait, &self.window_target);
            let start = Instant::now();
            let deadline = match self.window_target.control_flow() {
                ControlFlow::WaitUntil(when) => Some(when),
                _ => None,
            };
            let idle = !REDRAW.load(Ordering::Acquire) || background;
            if idle {
                let delay = deadline.map_or(Duration::from_millis(8), |when| {
                    when.saturating_duration_since(start).min(Duration::from_millis(8))
                });
                std::thread::sleep(delay);
            }
            cause = match deadline {
                Some(requested_resume) if Instant::now() >= requested_resume =>
                    StartCause::ResumeTimeReached { start, requested_resume },
                requested_resume if idle => StartCause::WaitCancelled { start, requested_resume },
                _ => StartCause::Poll,
            };
        }""")

    runtime = source / "crates/horizon-nx"
    build_rs = runtime / "build.rs"
    replace(build_rs, "    if target_os == \"horizon\" {", """    if target_os == "horizon" {
        println!("cargo:rerun-if-env-changed=DEVKITPRO");
        println!("cargo:rerun-if-env-changed=HORIZON_GCC_VERSION");
        let sdk = std::env::var("DEVKITPRO").expect("DEVKITPRO");
        let gcc = std::env::var("HORIZON_GCC_VERSION").expect("HORIZON_GCC_VERSION");""")
    build_rs.write_text(build_rs.read_text().replace("/opt/devkitpro", "{sdk}").replace("16.1.0/pic", "{gcc}/pic"))

    client = source / "crates/horizon-client/src"

    threads = source / "crates/horizon-threads"
    # The published workshop's mimalloc backend is separate. Use the same
    # newlib System allocator as our hardware-validated Rust client.
    replace(threads / "Cargo.toml", "[dependencies]\nmimalloc", "[target.'cfg(not(target_os = \"horizon\"))'.dependencies]\nmimalloc")
    replace(threads / "src/lib.rs", "pub use mimalloc::MiMalloc as Allocator;", """#[cfg(not(target_os = "horizon"))]
pub use mimalloc::MiMalloc as Allocator;
#[cfg(target_os = "horizon")]
pub use std::alloc::System as Allocator;""")
    replace(threads / "src/lib.rs", "const PURGE_DELAY:", '#[cfg(not(target_os = "horizon"))]\nconst PURGE_DELAY:')
    replace(threads / "src/lib.rs", '    let delay = if cfg!(target_os = "horizon") { -1 } else { 0 };', "")
    replace(threads / "src/lib.rs", "    unsafe {\n        libmimalloc_sys::mi_option_set(PURGE_DELAY, delay);", '    #[cfg(not(target_os = "horizon"))]\n    unsafe {\n        libmimalloc_sys::mi_option_set(PURGE_DELAY, 0);')
    replace(threads / "src/lib.rs", "    unsafe {\n        libmimalloc_sys::mi_collect(true);", '    #[cfg(not(target_os = "horizon"))]\n    unsafe {\n        libmimalloc_sys::mi_collect(true);')

    input_crate = source / "crates/horizon-client-input"
    manifest = input_crate / "Cargo.toml"
    line = next(line for line in manifest.read_text().splitlines() if line.startswith("arboard ="))
    replace(manifest, line + "\n", "")
    with manifest.open("a") as out:
        out.write('\n[target.\'cfg(not(target_os = "horizon"))\'.dependencies]\n' + line + "\n")
    shutil.copy2(compat / "crates/horizon-client-input/src/clipboard.rs", input_crate / "src/clipboard.rs")
    for name in ("horizon-client", "horizon-client-render"):
        manifest = source / "crates" / name / "Cargo.toml"
        replace(manifest, "[target.'cfg(unix)'.dependencies]", "[target.'cfg(all(unix, not(target_os = \"horizon\")))'.dependencies]")
    replace(source / "crates/horizon-client/Cargo.toml", 'horizon-nx = { path = "../horizon-nx" }',
            'horizon-nx = { path = "../horizon-nx" }\n'
            + 'horizon-platform = { path = ' + json.dumps(str(compat / "crates/horizon-platform")) + ' }')
    save = client / "save.rs"
    replace(save, 'pub fn estimate_disk_space(dir: &Path) -> u64 {\n    #[cfg(unix)]',
            'pub fn estimate_disk_space(dir: &Path) -> u64 {\n'
            '    #[cfg(target_os = "horizon")]\n'
            '    return horizon_platform::available_space(dir).unwrap_or(u64::MAX);\n'
            '    #[cfg(all(unix, not(target_os = "horizon")))]')
    for file in ("Cargo.toml", "src/direct.rs", "src/reach.rs"):
        name = Path("crates/horizon-friends") / file
        shutil.copy2(compat / name, source / name)

    config = build / "switch.toml"
    bp = bevy / "patches"
    deps = {name: bp / name for name in ("raw-window-handle", "cpal", "ring", "getrandom", "getrandom-0.2")}
    deps.update({"winit": winit, "libc": tests / "vendor/libc"})
    deps.update(patch_wgpu(patches))
    config.write_text("[env]\n"
                      f"HORIZON_GCC_VERSION = {json.dumps(gcc_version)}\n"
                      'CC_aarch64_nintendo_horizon = "aarch64-none-elf-gcc"\n'
                      'CFLAGS_aarch64_nintendo_horizon = "-march=armv8-a+crc+crypto -mtune=cortex-a57 -mtp=soft -fPIE"\n'
                      "\n[patch.crates-io]\n" + "".join(
                          f"{json.dumps(name)} = {{ path = {json.dumps(str(path))}"
                          + (', package = "getrandom"' if name == "getrandom-0.2" else "")
                          + " }\n" for name, path in deps.items()))
    shutil.copy2(bevy / "aarch64-nintendo-horizon.json", build)
    print(f"Prepared original Horizon-NX {NX} in {source}")
    print(f"Config: {config}")


if __name__ == "__main__":
    main()
