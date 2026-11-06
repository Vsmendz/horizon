from pathlib import Path
import re,json
base=Path(__file__).resolve().parent
root=base.parent/"vendor/libc"
original=next(Path("/home/c_vinwhi/.cargo/registry/src").glob("*/libc-0.2.191"))
abi=json.loads((base/"abi.json").read_text())
sw='all(target_os = "horizon", target_arch = "aarch64")'
guard=f"#[cfg({sw})]"
notguard=f"#[cfg(not({sw}))]"
p=root/"src/unix/newlib/mod.rs"; s=(original/"src/unix/newlib/mod.rs").read_text()
for name,ctype in {"blkcnt_t":"c_long","blksize_t":"c_long","suseconds_t":"c_long","pthread_t":"*mut c_void"}.items():
 pattern=rf"pub type {name} = [^;]+;"
 old=re.search(pattern,s).group()
 s=s.replace(old,notguard+"\n"+old+"\n"+guard+f"\npub type {name} = {ctype};",1)
s=s.replace('if #[cfg(any(target_os = "espidf", target_os = "vita"))] {',f'if #[cfg({sw})] {{\n        pub type dev_t = c_short;\n        pub type ino_t = c_ushort;\n        pub type off_t = c_long;\n    }} else if #[cfg(any(target_os = "espidf", target_os = "vita"))] {{',1)
s=s.replace('if #[cfg(target_os = "horizon")] {\n        pub type sa_family_t', 'if #[cfg(all(target_os = "horizon", not(target_arch = "aarch64")))] {\n        pub type sa_family_t',1)
# Replace pthread structures only on Switch; keep all old target branches intact.
new=[]
for name in "pthread_attr_t pthread_mutex_t pthread_mutexattr_t pthread_cond_t pthread_condattr_t pthread_rwlock_t pthread_rwlockattr_t".split():
 pattern=rf"    pub struct {name} \{{.*?\n    \}}"
 m=re.search(pattern,s,re.S); assert m,name
 s=s[:m.start()]+"    "+notguard+"\n"+m.group()+s[m.end():]
 field="__size" if name in ("pthread_attr_t","pthread_rwlockattr_t") else "size"
 new.append(f"    {guard}\n    #[repr(align({abi[name+'.align']}))]\n    pub struct {name} {{ {field}: [u8; {abi[name+'.size']}], }}")
s+="\ns! {\n"+"\n".join(new)+"\n}\n"
# Original cfg_attr alignment directives apply only to original (disabled) structs.
cond='pub const PTHREAD_COND_INITIALIZER: pthread_cond_t = pthread_cond_t {\n    size: [__PTHREAD_INITIALIZER_BYTE; __SIZEOF_PTHREAD_COND_T],\n};'
s=s.replace(cond,notguard+"\n"+cond+"\n"+guard+'\npub const PTHREAD_COND_INITIALIZER: pthread_cond_t = pthread_cond_t { size: [1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0] };')
# Per-target ABI sizes.
for name in ["ATTR","MUTEX","MUTEXATTR","COND","CONDATTR","RWLOCK","RWLOCKATTR"]:
 # Size constants referenced by generic initializers; define Switch branch ahead of existing selection.
 pass
branch='if #[cfg('+sw+')] {\n        const __PTHREAD_INITIALIZER_BYTE: u8 = 0;\n'
for name in ["ATTR","MUTEX","MUTEXATTR","COND","CONDATTR","RWLOCK","RWLOCKATTR"]:
 branch+=f"        pub const __SIZEOF_PTHREAD_{name}_T: usize = {abi['pthread_'+name.lower()+'_t.size']};\n"
branch+=f"        pub const __SIZEOF_PTHREAD_BARRIER_T: usize = {abi['pthread_barrier_t.size']};\n    }} else "
s=s.replace('if #[cfg(target_os = "espidf")] {\n        const __PTHREAD_INITIALIZER_BYTE',branch+'if #[cfg(target_os = "espidf")] {\n        const __PTHREAD_INITIALIZER_BYTE',1)
s=s.replace('} else if #[cfg(target_os = "horizon")] {\n        mod horizon;', '} else if #[cfg('+sw+')] {\n        mod switch;\n        pub use self::switch::*;\n    } else if #[cfg(target_os = "horizon")] {\n        mod horizon;',1)
# Platform constants differ from 3DS.
for name in ["SO_ERROR","TCP_NODELAY","IP_TOS","IP_TTL","O_CLOEXEC"]:
 pattern=rf"pub const {name}: ([^=]+)= ([^;]+);"
 def repl(m):
  return notguard+"\n        "+m.group()+"\n        "+guard+f"\n        pub const {name}: {m.group(1)}= {abi[name]};"
 s=re.sub(pattern,repl,s)
s=s.replace("const ULONG_SIZE: usize = 32;", notguard+"\nconst ULONG_SIZE: usize = 32;\n"+guard+"\nconst ULONG_SIZE: usize = 64;")
s=s.replace("pub const __SIZEOF_PTHREAD_BARRIERATTR_T: usize = 4;", notguard+"\npub const __SIZEOF_PTHREAD_BARRIERATTR_T: usize = 4;\n"+guard+"\npub const __SIZEOF_PTHREAD_BARRIERATTR_T: usize = 0;")
p.write_text(s)
unix=(original/"src/unix/mod.rs").read_text()
unix=unix.replace("    pub fn sysconf(name: c_int) -> c_long;", '    #[cfg_attr(all(target_os = "horizon", target_arch = "aarch64"), link_name = "horizon_switch_sysconf")]\n    pub fn sysconf(name: c_int) -> c_long;')
(root/"src/unix/mod.rs").write_text(unix)
# Switch-specific module, retaining upstream copyright/license files with the fork.
s=(original/"src/unix/newlib/horizon/mod.rs").read_text()
s=s.replace('//! ARMv6K Nintendo 3DS C Newlib definitions','//! AArch64 Nintendo Switch newlib/libnx definitions.\n// Adapted from libc 0.2.191 (MIT OR Apache-2.0), Rust Project Developers.\n// Layouts measured with devkitA64 GCC 15.2.0; see runtime-audit/abi.json.')
s=s.replace("pub h_addrtype: u16","pub h_addrtype: c_int").replace("pub h_length: u16","pub h_length: c_int")
s=s.replace("pub events: c_int","pub events: c_short").replace("pub revents: c_int","pub revents: c_short")
s=s.replace("pub struct sockaddr {\n", "pub struct sockaddr {\n        pub sa_len: c_uchar,\n").replace("[c_char; 26usize]","[c_char; 14usize]")
s=re.sub(r"pub struct sockaddr_storage \{.*?\n    \}", """pub struct sockaddr_storage {
        pub ss_len: c_uchar,
        pub ss_family: crate::sa_family_t,
        __ss_pad1: Padding<[c_char; 6]>,
        __ss_align: i64,
        __ss_pad2: Padding<[c_char; 112]>,
    }""",s,flags=re.S)
s=s.replace("pub struct sockaddr_in {\n", "pub struct sockaddr_in {\n        pub sin_len: c_uchar,\n")
for name in ["AF_INET6","SOL_SOCKET"]:
 s=re.sub(rf"pub const {name}: c_int = [^;]+;",f"pub const {name}: c_int = {abi[name]};",s)
for name,value in abi.items():
 if name.startswith("POLL"):
  s=re.sub(rf"pub const {name}: c_int = [^;]+;",f"pub const {name}: c_short = {value};",s)
s=re.sub(r"    pub struct stat \{.*?\n    \}\n", "", s, flags=re.S)
s=s.replace("use crate::off_t;\n","")
(root/"src/unix/newlib/switch.rs").write_text(s)
print("Created isolated AArch64 branch; upstream 3DS module preserved byte-for-byte.")
assert (root/"src/unix/newlib/horizon/mod.rs").read_bytes()==(original/"src/unix/newlib/horizon/mod.rs").read_bytes()
