from pathlib import Path
import json
root=Path(__file__).resolve().parent
a=json.loads((root/"abi.json").read_text())
lines=["// Generated from SDK C measurements; not from Rust bindings."]
unsupported_types={"pthread_barrier_t","pthread_barrierattr_t"}
for k,v in a.items():
 if ".is." in k: continue
 if k.endswith(".size") or k.endswith(".align"):
  t,kind=k.rsplit(".",1); t=t.replace("struct ","")
  if t in unsupported_types: continue
  lines.append(f'const _: () = assert!(core::mem::{"size_of" if kind=="size" else "align_of"}::<libc::{t}>() == {v});')
 elif "." in k:
  t,f=k.split("."); lines.append(f'const _: () = assert!(core::mem::offset_of!(libc::{t}, {f}) == {v});')
 else: lines.append(f'const _: () = assert!(libc::{k} as u64 == {v});')
initializers=json.loads((root/"abi_initializers.json").read_text())
for name,data in initializers.items():
 n=len(data)
 lines.append(f'const _: () = {{ let bytes: [u8; {n}] = unsafe {{ core::mem::transmute(libc::PTHREAD_{name.upper()}_INITIALIZER) }}; let expected = {data}; let mut i = 0; while i < {n} {{ assert!(bytes[i] == expected[i]); i += 1; }} }};')
(root/"abi_assertions.rs").write_text("\n".join(lines)+"\n")
print(len(lines)-1,"independent compile-time ABI assertions generated.")
