#!/usr/bin/env bash
# WSL adaptation of bevy-horizon's scripts/build_nvk.sh (MIT OR Apache-2.0).
# Expects pinned Mesa and bevy-horizon checkouts and a host tools prefix.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
mesa="$(realpath "$1")"
bevy="$(realpath "$2")"
tools="$(realpath "$3")"
sdk="$(realpath "$DEVKITPRO")"
revision=606441c90e459028a50fca352daebfc20211f890
test "$(git -C "$mesa" rev-parse HEAD)" = "$revision"
test "$(git -C "$bevy" rev-parse HEAD)" = 3eac779b232ee47cd0eb957676c64386ac7b0f1d
cd "$mesa"
work="$mesa/.switch-hybrid-tools"
mkdir -p "$work/include" "$work/pkgconfig"
if [[ ! -e "$work/sdk" ]]; then ln -s "$sdk" "$work/sdk"; fi
test "$(realpath "$work/sdk")" = "$sdk"
export DEVKITPRO="$work/sdk"
export PATH="$tools/bin:$PATH"
export MESA_SWITCH_RUSTC="$(command -v rustc)"
export MESA_SWITCH_BINDGEN="$(command -v bindgen)"
export MESA_SWITCH_RUST_TARGET=aarch64-unknown-linux-gnu
cp bindgen-switch-wrapper.sh "$work/bindgen"
cp rustc-switch-wrapper.sh "$work/rustc"
cp bindgen-atomic-shim.h "$work/"
cp src/gallium/winsys/nouveau/drm/nouveau.h src/nouveau/headers/nv_device_info.h "$work/include/"
chmod +x "$work/bindgen" "$work/rustc"
for pc in "$DEVKITPRO"/portlibs/switch/lib/pkgconfig/*.pc; do
    sed "s|/opt/devkitpro|$DEVKITPRO|g" "$pc" > "$work/pkgconfig/${pc##*/}"
done
cat > "$work/pkg-config" <<EOF
#!/bin/sh
export PKG_CONFIG_LIBDIR='$work/pkgconfig'
unset PKG_CONFIG_PATH
exec pkg-config "\$@"
EOF
chmod +x "$work/pkg-config"
# Meson reuses the C link flags for its Rust linker check. The GNU-target
# NAK/NIL staticlibs can use the cross GCC linker for that check.
sed -e "s|/opt/devkitpro/portlibs/switch/bin/aarch64-none-elf-pkg-config|$work/pkg-config|g" \
    -e "s|/opt/devkitpro|$DEVKITPRO|g" -e "s|/usr/local/libexec|$work|g" \
    -e "/^rust =/a rust_ld = 'aarch64-linux-gnu-gcc'" \
    -e "/link_args/s/,'-mtp=soft'//g" \
    -e "s|'-D__SWITCH__'|'-I$work/include','-D__SWITCH__'|g" \
    switch_cross_file.txt > "$work/cross.ini"
printf "[binaries]\nrust = '%s'\nc = '/usr/bin/gcc'\ncpp = '/usr/bin/g++'\nllvm-config = '%s'\n" "$MESA_SWITCH_RUSTC" "$(command -v llvm-config-15)" > "$work/native.ini"
# Verify the full stack on a clean source tree once before mutating the checkout.
if [[ ! -f "$work/patches.sha256" ]]; then
    test -z "$(git diff --name-only)"
    for patch in "$bevy"/patches/mesa-switch/*.patch; do git apply "$patch"; done
    sha256sum "$bevy"/patches/mesa-switch/*.patch > "$work/patches.sha256"
fi
sha256sum --check "$work/patches.sha256"
export PKG_CONFIG_PATH="$tools/lib/pkgconfig"
export LLVM_CONFIG="$(command -v llvm-config-15)"
setup() {
    local dir="$1"; shift
    if [[ ! -f "$dir/meson-private/coredata.dat" ]]; then meson setup "$dir" "$@"; else meson setup --reconfigure "$dir" "$@"; fi
}
setup builddir-native-gcc --native-file "$work/native.ini" \
    -Dvulkan-drivers= -Dgallium-drivers= -Dshader-cache=enabled -Dplatforms= \
    -Dglx=disabled -Degl=disabled -Dopengl=false -Dgles1=disabled -Dgles2=disabled \
    '-Dtools=[]' -Dllvm=enabled -Dmesa-clc=enabled -Dprecomp-compiler=enabled -Dinstall-mesa-clc=true
ninja -C builddir-native-gcc -j "${JOBS:-6}" src/compiler/clc/mesa_clc src/compiler/spirv/vtn_bindgen2
export PATH="$mesa/builddir-native-gcc/src/compiler/clc:$mesa/builddir-native-gcc/src/compiler/spirv:$PATH"
setup builddir-switch --cross-file "$work/cross.ini" --native-file "$work/native.ini" \
    --default-library=static --buildtype=release -Doptimization=2 -Db_lto=false -Db_ndebug=true \
    -Dvulkan-drivers=nouveau -Dgallium-drivers= -Dgallium-rusticl=false -Dplatforms=switch \
    -Dglx=disabled -Degl=disabled -Dopengl=false -Dgles1=disabled -Dgles2=disabled -Dvideo-codecs= \
    -Dshader-cache=enabled -Dxmlconfig=enabled -Dexpat=enabled '-Dtools=[]' -Dllvm=disabled \
    -Dshared-glapi=disabled -Dshared-llvm=disabled -Dmesa-clc=system -Dprecomp-compiler=system \
    -Dcpp_rtti=false -Dbuild-tests=false -Dnvk-build-id="$revision"
ninja -C builddir-switch -j "${JOBS:-6}" src/nouveau/vulkan/libvulkan.a
aarch64-none-elf-ranlib builddir-switch/src/nouveau/vulkan/libvulkan.a
mkdir -p "$bevy/nvk/lib"
cp builddir-switch/src/nouveau/vulkan/libvulkan.a "$bevy/nvk/lib/"
bash "$here/localize-nvk.sh" "$bevy/nvk/lib/libvulkan.a"
