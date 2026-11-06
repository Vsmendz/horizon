import os, subprocess, json, struct
from pathlib import Path
root=Path(__file__).resolve().parent
sdk=Path(os.environ["DEVKITPRO"])
work=Path("/tmp/horizon-switch-target/abi"); work.mkdir(parents=True,exist_ok=True)
types="off_t dev_t ino_t mode_t nlink_t uid_t gid_t blksize_t blkcnt_t time_t clock_t clockid_t suseconds_t sa_family_t socklen_t nfds_t pthread_t pthread_key_t pthread_attr_t pthread_mutex_t pthread_mutexattr_t pthread_cond_t pthread_condattr_t pthread_rwlock_t pthread_rwlockattr_t pthread_barrier_t pthread_barrierattr_t fd_set".split()
structs={"stat":"st_dev st_ino st_mode st_nlink st_uid st_gid st_rdev st_size st_atim st_mtim st_ctim st_blksize st_blocks st_spare4","timespec":"tv_sec tv_nsec","timeval":"tv_sec tv_usec","dirent":"d_ino d_type d_name","pollfd":"fd events revents","hostent":"h_name h_aliases h_addrtype h_length h_addr_list","sockaddr":"sa_len sa_family sa_data","sockaddr_storage":"ss_len ss_family","sockaddr_in":"sin_len sin_family sin_port sin_addr sin_zero","sockaddr_in6":"sin6_family sin6_port sin6_flowinfo sin6_addr sin6_scope_id","addrinfo":"ai_flags ai_family ai_socktype ai_protocol ai_addrlen ai_canonname ai_addr ai_next"}
expr={}
for t in types+["struct "+s for s in structs]:
 expr[t+".size"]="sizeof("+t+")"; expr[t+".align"]="_Alignof("+t+")"
for t in types[:17]:
 for c in ["char","unsigned char","short","unsigned short","int","unsigned int","long","unsigned long","long long","unsigned long long"]:
  expr[t+".is."+c]="__builtin_types_compatible_p("+t+","+c+")"
for s,fields in structs.items():
 for f in fields.split(): expr[s+"."+f]="offsetof(struct "+s+","+f+")"
const="PTHREAD_MUTEX_NORMAL PTHREAD_MUTEX_RECURSIVE PTHREAD_MUTEX_ERRORCHECK CLOCK_REALTIME CLOCK_MONOTONIC O_RDONLY O_WRONLY O_RDWR O_CREAT O_TRUNC O_APPEND O_CLOEXEC O_NONBLOCK FD_SETSIZE AF_INET AF_INET6 SOCK_STREAM SOCK_DGRAM SOL_SOCKET SO_ERROR TCP_NODELAY IP_TOS IP_TTL _SC_PAGESIZE POLLIN POLLPRI POLLOUT POLLRDNORM POLLWRNORM POLLRDBAND POLLWRBAND POLLERR POLLHUP POLLNVAL".split()
for c in const: expr[c]=c
headers=["stddef.h","sys/types.h","sys/stat.h","sys/time.h","dirent.h","pthread.h","poll.h","sys/socket.h","netinet/in.h","netinet/tcp.h","netdb.h","fcntl.h","limits.h"]
source="".join("#include <"+h+">\n" for h in headers)
source+="const unsigned long long measurements[] = {\n"+",\n".join("(unsigned long long)("+e+")" for e in expr.values())+"\n};\n"
for kind,typename,initializer in [("mutex","pthread_mutex_t","PTHREAD_MUTEX_INITIALIZER"),("cond","pthread_cond_t","PTHREAD_COND_INITIALIZER"),("rwlock","pthread_rwlock_t","PTHREAD_RWLOCK_INITIALIZER")]:
 source+=f'const {typename} init_{kind} __attribute__((section(".abi_{kind}"))) = {initializer};\n'
(root/"abi_probe.c").write_text(source)
cc=sdk/"devkitA64/bin/aarch64-none-elf-gcc"
subprocess.run([str(cc),"-D__SWITCH__","-I"+str(sdk/"libnx/include"),"-c",str(root/"abi_probe.c"),"-o",str(work/"abi.o")],check=True)
subprocess.run([str(sdk/"devkitA64/bin/aarch64-none-elf-objcopy"),"-O","binary","--only-section=.rodata",str(work/"abi.o"),str(work/"abi.bin")],check=True)
raw=(work/"abi.bin").read_bytes()
assert len(raw)==8*len(expr),(len(raw),len(expr))
data=dict(zip(expr,struct.unpack("<"+"Q"*len(expr),raw)))
(root/"abi.json").write_text(json.dumps(data,indent=2))
for t in types:
 print(t,data[t+".size"],data[t+".align"],[k.split(".is.")[1] for k,v in data.items() if k.startswith(t+".is.") and v])
print("Measured",len(expr),"ABI values from cross-compiled C object.")

initializers={}
for kind in ("mutex","cond","rwlock"):
 target=work/(kind+".bin")
 subprocess.run([str(sdk/"devkitA64/bin/aarch64-none-elf-objcopy"),"-O","binary","--only-section=.abi_"+kind,str(work/"abi.o"),str(target)],check=True)
 initializers[kind]=list(target.read_bytes())
(root/"abi_initializers.json").write_text(json.dumps(initializers,indent=2))
print("C initializer bytes", initializers)
