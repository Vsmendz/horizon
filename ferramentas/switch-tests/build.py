import json, os, sys, subprocess, shlex, shutil
from pathlib import Path
src=Path(__file__).resolve().parent
real_sdk=Path(os.environ["DEVKITPRO"]).resolve()
sdk=Path("/tmp/horizon-devkitpro")
if sdk.is_symlink() and sdk.resolve()!=real_sdk: raise SystemExit("SDK alias points elsewhere")
if not sdk.exists(): sdk.symlink_to(real_sdk, target_is_directory=True)
build=Path("/tmp/horizon-switch-target/native")
build.mkdir(parents=True,exist_ok=True)
out=src/"out"; out.mkdir(exist_ok=True)
env=os.environ.copy(); env["DEVKITPRO"]=str(sdk)
name=sys.argv[1] if len(sys.argv)>1 else "test1_triangle"
recorded=json.loads((src/"results.json").read_text(encoding="utf-8"))["results"]
if (out/(name+".nro")).exists() and any(
    item["test"] == name and item.get("hardware", "").startswith("user_confirmed")
    for item in recorded
):
    raise SystemExit(f"{name}.nro was validated on hardware; choose a new artifact name")
log=(out/(name+".build.log")).open("w")
def run(args):
    line=shlex.join(map(str,args)); print(line,flush=True); log.write("$ "+line+"\n"); log.flush()
    p=subprocess.run(list(map(str,args)),env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    print(p.stdout,end="",flush=True); log.write(p.stdout); log.flush()
    if p.returncode: raise SystemExit(p.returncode)
is_std=name=="test2c_rust_std"
is_integration=name=="test3_horizon_integration"
is_renderer=name=="test4_horizon_renderer"
is_worldgen=name=="test7_vanilla_worldgen"
is_diagnostic=name in ("test8b_vanilla_render_fix", "test8c_gles_pipeline_probe", "test8d_gles_atlas_probe", "test8e_gles_atlas_fix")
is_vanilla=name=="test8_vanilla_streaming" or is_diagnostic
is_client=name in (
    "test5_horizon_client", "test5b_horizon_client_fix",
    "test6_horizon_streaming", "test6b_horizon_streaming_async",
    "test6c_horizon_nxlink_log",
    "test8_vanilla_streaming",
    "test8b_vanilla_render_fix",
    "test8c_gles_pipeline_probe",
    "test8d_gles_atlas_probe",
    "test8e_gles_atlas_fix",
)
is_rust=name in ("test2a_rust", "test2b_rust_egl") or is_std or is_integration or is_renderer or is_client or is_worldgen
is_c=name=="test2c_posix_c" or is_rust
cc=sdk/("devkitA64/bin/aarch64-none-elf-gcc" if is_c else "devkitA64/bin/aarch64-none-elf-g++")
flags=["-march=armv8-a+crc+crypto","-mtune=cortex-a57","-mtp=soft","-fPIE","-D__SWITCH__","-O2","-g","-ffunction-sections","-fdata-sections","-Wall",
       "-I"+str(sdk/"libnx/include"),"-I"+str(sdk/"portlibs/switch/include")]
if not is_c: flags += ["-fno-rtti","-fno-exceptions"]
if is_client or is_worldgen: flags += [f'-DHORIZON_CLIENT_TEST_NAME="{name}"']
if is_worldgen: flags += ["-DHORIZON_WORLDGEN_PROBE"]
if is_vanilla: flags += ["-DHORIZON_VANILLA_CLIENT"]
has_network_log=name=="test6c_horizon_nxlink_log"
if has_network_log: flags += ["-DHORIZON_NXLINK_LOG"]
obj=build/(name+".o"); elf=build/(name+".elf")
source=src/("client_entry.c" if is_client or is_worldgen else "renderer_entry.c" if is_renderer else "horizon_entry.c" if is_integration else "std_entry.c" if is_std else "rust_entry.c" if is_rust else name+(".c" if is_c else ".cpp"))
extra=[]
if is_renderer or is_client:
    run([sys.executable, src/"prepare_switch_assets.py", *(["--vanilla"] if is_vanilla else [])])
if is_vanilla:
    source_bundle=out/"test7-worldgen-romfs"
    if not (source_bundle/"worldgen/block_flags.json").is_file():
        raise SystemExit("Prepare the 26.3 worldgen bundle with prepare_worldgen_probe.py first")
    stage=out/"test8-romfs"
    for file in ("facts.json", "block-behaviours.json"):
        shutil.copy2(source_bundle/file, stage)
    shutil.copytree(source_bundle/"worldgen", stage/"worldgen", dirs_exist_ok=True)
if is_rust:
    env["CARGO_TARGET_DIR"]="/tmp/horizon-switch-target"
    env["RUSTFLAGS"]="-C relocation-model=pic"
    cargo=[Path.home()/".cargo/bin/cargo","build","--manifest-path",src/"rust/Cargo.toml","--release",
           "--target","aarch64-nintendo-switch-freestanding","-Zbuild-std=core,alloc",
           "-Zbuild-std-features=compiler-builtins-mem"]
    if name=="test2b_rust_egl": cargo+=["--features","egl"]
    if is_std:
        cargo=[Path.home()/".cargo/bin/cargo","--config",src/"runtime-audit/cargo-switch.toml",
               "build","--manifest-path",src/"runtime-audit/Cargo.toml","--release",
               "--target",src/"runtime-audit/aarch64-nintendo-switch-newlib.json",
               "-Zjson-target-spec","-Zbuild-std=core,alloc,std,panic_abort",
               "-Zbuild-std-features=compiler-builtins-mem"]
    if is_integration:
        cargo=[Path.home()/".cargo/bin/cargo","--config",src/"horizon-integration/cargo-switch.toml",
               "build","--manifest-path",src/"horizon-integration/Cargo.toml","--release",
               "--target",src/"runtime-audit/aarch64-nintendo-switch-newlib.json",
               "-Zjson-target-spec","-Zbuild-std=core,alloc,std,panic_abort",
               "-Zbuild-std-features=compiler-builtins-mem"]
    if is_renderer:
        cargo=[Path.home()/".cargo/bin/cargo","--config",src/"horizon-renderer/cargo-switch.toml",
               "build","--manifest-path",src/"horizon-renderer/Cargo.toml","--release",
               "--target",src/"runtime-audit/aarch64-nintendo-switch-newlib.json",
               "-Zjson-target-spec","-Zbuild-std=core,alloc,std,panic_abort",
               "-Zbuild-std-features=compiler-builtins-mem"]
    if is_client:
        cargo=[Path.home()/".cargo/bin/cargo","--config",src/"horizon-client-switch/cargo-switch.toml",
               "build","--manifest-path",src/"horizon-client-switch/Cargo.toml","--release",
               "--target",src/"runtime-audit/aarch64-nintendo-switch-newlib.json",
               "-Zjson-target-spec","-Zbuild-std=core,alloc,std,panic_abort",
               "-Zbuild-std-features=compiler-builtins-mem"]
        if is_vanilla: cargo+=["--features", "vanilla"]
        if is_diagnostic: cargo+=["--features", "render-diagnostics"]
    if is_worldgen:
        probe=src/"horizon-worldgen-switch"
        cargo=[Path.home()/".cargo/bin/cargo","--config",src/"horizon-integration/cargo-switch.toml",
               "build","--lib","--manifest-path",probe/"Cargo.toml","--release",
               "--target",src/"runtime-audit/aarch64-nintendo-switch-newlib.json",
               "-Zjson-target-spec","-Zbuild-std=core,alloc,std,panic_abort",
               "-Zbuild-std-features=compiler-builtins-mem"]
    run(cargo)
    extra=[Path(env["CARGO_TARGET_DIR"])/"aarch64-nintendo-switch-freestanding/release/libhorizon_switch_probes.a"]
if is_std: extra=[Path(env["CARGO_TARGET_DIR"])/"aarch64-nintendo-switch-newlib/release/libhorizon_switch_std_audit.a"]
if is_integration: extra=[Path(env["CARGO_TARGET_DIR"])/"aarch64-nintendo-switch-newlib/release/libhorizon_switch_integration.a"]
if is_renderer: extra=[Path(env["CARGO_TARGET_DIR"])/"aarch64-nintendo-switch-newlib/release/libhorizon_switch_renderer.a"]
if is_client: extra=[Path(env["CARGO_TARGET_DIR"])/"aarch64-nintendo-switch-newlib/release/libhorizon_switch_client.a"]
if is_worldgen: extra=[Path(env["CARGO_TARGET_DIR"])/"aarch64-nintendo-switch-newlib/release/libhorizon_switch_worldgen.a"]
run([cc,*flags,"-c",source,"-o",obj])
if has_network_log:
    log_obj=build/(name+"_nxlink.o")
    run([cc,*flags,"-c",src/"nxlink_log.c","-o",log_obj])
    extra.insert(0,log_obj)
libs=["-lnx"]
if name in ("test1_triangle","test2b_rust_egl"): libs=["-lglad","-lEGL","-lglapi","-ldrm_nouveau","-lnx"]
if name=="test2c_posix_c" or is_std or is_worldgen: libs=["-lpthread","-lnx"]
if is_integration or is_renderer or is_client: libs=["-lEGL","-lglapi","-ldrm_nouveau","-lpthread","-lnx"]
# Mesa's static archive uses the C++ runtime even when the entry is C/Rust.
linker=sdk/"devkitA64/bin/aarch64-none-elf-g++"
run([linker,*flags,"-specs="+str(sdk/"libnx/switch.specs"),obj,*extra,
     "-L"+str(sdk/"libnx/lib"),"-L"+str(sdk/"portlibs/switch/lib"),
     "-Wl,-Map,"+str(build/(name+".map")),*libs,"-o",elf])
run([sdk/"tools/bin/nacptool","--create",name,"Horizon port test","0.1.0",build/(name+".nacp")])
nro_args=[sdk/"tools/bin/elf2nro",elf,out/(name+".nro"),"--nacp="+str(build/(name+".nacp")),"--icon="+str(sdk/"libnx/default_icon.jpg")]
if is_vanilla:
    nro_args.append("--romfsdir="+str(out/"test8-romfs"))
elif is_renderer or is_client:
    nro_args.append("--romfsdir="+str(src/"out/sdcard/switch/horizon"))
if is_worldgen:
    worldgen_romfs=src/"out/test7-worldgen-romfs"
    if not (worldgen_romfs/"expected.txt").is_file():
        raise SystemExit("Stage the prepared worldgen bundle and the Java oracle fixture first")
    nro_args.append("--romfsdir="+str(worldgen_romfs))
run(nro_args)
shutil.copy2(elf, out/elf.name)
shutil.copy2(build/(name+".map"), out/(name+".map"))
print("COMPILED AND LINKED; AWAITING HARDWARE VALIDATION",flush=True)
