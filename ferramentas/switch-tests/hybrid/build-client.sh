#!/usr/bin/env bash
# Link the actual Horizon-NX entry point with the localized static NVK driver.
set -euo pipefail
build="$(realpath "$1")"
export HORIZON_NX_NVK_LIB="$(realpath "$2")"
history="$(realpath "$3")"
here="$(cd "$(dirname "$0")" && pwd)"
name=horizon_switch_hybrid
out="$here/../out"
mkdir -p "$out"
test -s "$HORIZON_NX_NVK_LIB/libvulkan_bh.a"
for ext in elf nro; do
    if [[ -e "$out/$name.$ext" ]]; then echo "Refusing to overwrite $out/$name.$ext" >&2; exit 1; fi
done
export CARGO_TARGET_DIR=/tmp/horizon-switch-target
export RUSTFLAGS='--cfg getrandom_backend="custom"'
cd "$build/source"
cargo --config "$build/switch.toml" build --profile fast \
    -p horizon-client --bin horizon-client --features horizon-nx/vulkan \
    --target "$build/aarch64-nintendo-horizon.json" -Zjson-target-spec \
    -Zbuild-std=core,alloc,std,panic_abort -Zbuild-std-features=compiler-builtins-mem -j "${JOBS:-4}"

stage="$(mktemp -d "$build/package.XXXXXX")"
mkdir "$stage/romfs"
{
    echo 'Original Horizon-NX revision: 9ee0e68f703c53861dc4891f4d4976c6dd34dd05'
    echo 'bevy-horizon revision: 3eac779b232ee47cd0eb957676c64386ac7b0f1d'
    echo 'Mesa base: 606441c90e459028a50fca352daebfc20211f890'
    sha256sum "$HORIZON_NX_NVK_LIB/libvulkan_bh.a" | awk '{print "NVK SHA-256: "$1}'
    rustc -V
} > "$stage/romfs/BUILD_INFO.txt"
cp "$CARGO_TARGET_DIR/aarch64-nintendo-horizon/fast/horizon-client" "$stage/$name.elf"
# These symbols distinguish this build from a crate or renderer probe.
nm_output="$stage/linked-symbols.txt"
aarch64-none-elf-nm --defined-only "$stage/$name.elf" > "$nm_output"
for symbol in main userAppInit vkGetInstanceProcAddr vkCreateViSurfaceNN; do
    if ! grep -Eq "[[:space:]]$symbol\$" "$nm_output"; then
        echo "Required client or NVK symbol missing: $symbol" >&2
        exit 1
    fi
done
nacptool --create "Horizon Switch Hybrid" "Horizon contributors" "0.1.0" "$stage/app.nacp"
elf2nro "$stage/$name.elf" "$stage/$name.nro" \
    --nacp="$stage/app.nacp" --icon="$DEVKITPRO/libnx/default_icon.jpg" --romfsdir="$stage/romfs"
audit_args=()
if [[ "${ALLOW_MISSING_HISTORY:-0}" = 1 ]]; then audit_args+=(--allow-missing-history); fi
python3 "$here/../verify_artifact.py" "$name" "$stage/romfs" \
    --artifacts "$stage" --history-root "$history" "${audit_args[@]}" | tee "$stage/$name.offline.log"
cp --no-clobber "$stage/$name."{elf,nro,offline.log} "$out/"
