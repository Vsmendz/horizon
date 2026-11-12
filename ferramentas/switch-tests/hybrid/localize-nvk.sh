#!/usr/bin/env bash
# Native WSL equivalent of John Uzoka's bevy-horizon/scripts/localize_nvk.sh.
# MIT OR Apache-2.0. Keep Mesa's embedded Rust and C runtime symbols private.
set -euo pipefail
archive="$(realpath "$1")"
out="$(dirname "$archive")"
work="$(mktemp -d "$out/localize.XXXXXX")"
keep='^(vk[A-Z]|nouveau_horizon_region_(alloc|free|stats)$|nouveau_horizon_tune$)'
aarch64-none-elf-nm -g --defined-only "$archive" |
    awk -v re="$keep" 'NF == 3 && $2 ~ /^[TDRBVW]$/ && $3 ~ re {print $3}' |
    sort -u > "$work/keep.txt"
grep -qx vkGetInstanceProcAddr "$work/keep.txt"
mapfile -t symbols < "$work/keep.txt"
args=()
for symbol in "${symbols[@]}"; do args+=(-u "$symbol"); done
aarch64-none-elf-ld -r --force-group-allocation -o "$work/vulkan_bh.o" "${args[@]}" "$archive"
aarch64-none-elf-objcopy --keep-global-symbols="$work/keep.txt" "$work/vulkan_bh.o"
aarch64-none-elf-nm -g --defined-only "$work/vulkan_bh.o" |
    awk -v re="$keep" '$3 !~ re {print}' > "$work/leftover.txt"
test ! -s "$work/leftover.txt"
aarch64-none-elf-readelf -g "$work/vulkan_bh.o" > "$work/sections.txt"
if grep -q 'COMDAT group' "$work/sections.txt"; then
    echo "NVK localization left COMDAT groups" >&2
    exit 1
fi
aarch64-none-elf-nm -u "$work/vulkan_bh.o" | awk '{print $NF}' | sort -u > "$out/undefined_bh.txt"
aarch64-none-elf-ar rcs "$work/libvulkan_bh.a" "$work/vulkan_bh.o"
mv "$work/libvulkan_bh.a" "$out/libvulkan_bh.a"
sha256sum "$out/libvulkan_bh.a"
