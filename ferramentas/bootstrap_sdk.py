import urllib.request, json, hashlib, tarfile, os, shutil
from pathlib import Path
repo = Path.cwd()
if (repo/"ferramentas/devkitpro/devkitA64/bin/aarch64-none-elf-g++").is_file() and (repo/"ferramentas/devkitpro/portlibs/switch/lib/libEGL.a").is_file():
    print("SDK already installed; leaving it intact.", flush=True)
    raise SystemExit(0)
cache = Path("/tmp/horizon-switch-sdk")
cache.mkdir(exist_ok=True)
def get(u, headers={}):
    return urllib.request.urlopen(urllib.request.Request(u, headers=headers), timeout=120)
token = json.load(get("https://auth.docker.io/token?service=registry.docker.io&scope=repository:devkitpro/devkita64:pull"))["token"]
headers = {"Authorization": "Bearer "+token, "Accept": "application/vnd.oci.image.manifest.v1+json"}
base = "https://registry-1.docker.io/v2/devkitpro/devkita64/"
digest = "sha256:82575ea78651b530b2e232bb3799cfd1fe331514e053d5f724bb4b28191fb79d"
manifest = json.load(get(base+"manifests/"+digest, headers))
(repo/"ferramentas/sdk-image.json").write_text(json.dumps({"image":"devkitpro/devkita64", "digest":digest,"manifest":manifest},indent=2))
for layer in manifest["layers"][1:]:
    sha = layer["digest"].split(":")[1]
    archive = cache/(sha+".tar.gz")
    print("Downloading", layer["size"], sha, flush=True)
    if not archive.exists():
        with get(base+"blobs/"+layer["digest"],headers) as response, archive.open("wb") as out:
            while block := response.read(4*1024*1024): out.write(block)
    if hashlib.file_digest(archive.open("rb"),"sha256").hexdigest()!=sha:
        raise SystemExit("Invalid layer checksum: "+str(archive))
    print("Verified; extracting SDK in /tmp", flush=True)
    with tarfile.open(archive) as tar:
        members = [m for m in tar.getmembers() if m.name.startswith("opt/devkitpro/") and "/.wh." not in m.name]
        tar.extractall(cache, members=members, filter="data")
    print("Extracted",len(members),"entries",flush=True)
dest = repo/"ferramentas/devkitpro"
if dest.is_dir() and not dest.is_symlink():
    dest.rmdir() # only succeeds if empty
if dest.is_symlink(): dest.unlink()
if not dest.exists(): shutil.move(str(cache/"opt/devkitpro"), str(dest))
print("SDK linked:",dest,"->",dest.resolve(),flush=True)
