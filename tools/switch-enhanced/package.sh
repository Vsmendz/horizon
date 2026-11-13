#!/usr/bin/env bash
# Package only the Enhanced executable and its provenance record.
set -euo pipefail
elf="$(realpath "$1")"
out="$(realpath -m "$2")"
nvk="$(realpath "$3")"
repo="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
name=horizon_nx_enhanced
mkdir -p "$out"
test ! -e "$out/$name.nro"
stage="$(mktemp -d)"
mkdir "$stage/romfs"
git -C "$repo" rev-parse HEAD > "$stage/romfs/BUILD_INFO.txt"
sha256sum "$nvk" | cut -d ' ' -f 1 >> "$stage/romfs/BUILD_INFO.txt"
bin="$DEVKITPRO/devkitA64/bin/aarch64-none-elf"
tools="$DEVKITPRO/tools/bin"
cp "$elf" "$stage/$name.elf"
"$bin-nm" --defined-only "$elf" > "$stage/symbols.txt"
for symbol in main userAppInit framebufferCreate enhanced_touch_sample vkGetInstanceProcAddr vkCreateViSurfaceNN; do
    grep -Eq "[[:space:]]$symbol\$" "$stage/symbols.txt"
done
"$tools/nacptool" --create 'Horizon NX Enhanced' 'Horizon contributors' '0.1.0' "$stage/app.nacp"
"$tools/elf2nro" "$elf" "$stage/$name.nro" --nacp="$stage/app.nacp" \
    --icon="$DEVKITPRO/libnx/default_icon.jpg" --romfsdir="$stage/romfs"
python3 - "$elf" "$stage/$name.nro" "$bin" "$stage/romfs/BUILD_INFO.txt" <<'PY' | tee "$stage/$name.verify.log"
import hashlib, pathlib, struct, subprocess, sys
elf,nro,tool,provenance=sys.argv[1:]
def readelf(*args):
    return subprocess.check_output([tool+"-readelf",*args,elf],text=True)
assert "AArch64" in readelf("-h")
assert "NEEDED" not in readelf("-d") and "INTERP" not in readelf("-Wl")
assert not any(len(s)>=8 and s[6]=="UND" for s in map(str.split,readelf("-W","--dyn-syms").splitlines()))
assert not any(len(s)>=8 and s[6]=="UND" and s[4]=="GLOBAL" for s in map(str.split,readelf("-Ws").splitlines()))
assert all("R_AARCH64_RELATIVE" in l for l in readelf("-Wr").splitlines() if "R_AARCH64_" in l)
b=pathlib.Path(nro).read_bytes()
assert b[16:20]==b"NRO0"
end,=struct.unpack_from("<I",b,24)
assert 0x80<=end<len(b) and b[end:end+4]==b"ASET"
for offset in (32,40,48):
    start,size=struct.unpack_from("<II",b,offset)
    assert start+size<=end
ranges=struct.unpack_from("<6Q",b,end+8)
for offset,size in zip(ranges[::2],ranges[1::2]):
    assert offset>=56 and size>0 and end+offset+size<=len(b)
offset,size=ranges[4:]
r=b[end+offset:end+offset+size]
header=struct.unpack_from("<10Q",r)
assert header[0]==80
for i in (1,3,5,7):
    assert header[i]+header[i+1]<=len(r)
dirs,files,contents=header[3],header[7],header[9]
_,sibling,child,file,_,_=struct.unpack_from("<6I",r,dirs)
assert sibling==child==0xffffffff
_,sibling,start,length,_,n=struct.unpack_from("<IIQQII",r,files+file)
assert sibling==0xffffffff and r[files+file+32:files+file+32+n]==b"BUILD_INFO.txt"
assert r[contents+start:contents+start+length]==pathlib.Path(provenance).read_bytes()
print("AArch64 ELF, no required undefined symbols or dynamic imports, NRO0/ASET and RomFS verified")
for file in (elf,nro):
    p=pathlib.Path(file)
    print(p.name,p.stat().st_size,hashlib.sha256(p.read_bytes()).hexdigest())
PY
cp "$stage/$name."{elf,nro,verify.log} "$out/"
