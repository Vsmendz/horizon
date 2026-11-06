include!("abi_assertions.rs");
use std::sync::{Arc, Mutex, Condvar};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
static TLS_DROPS: AtomicUsize = AtomicUsize::new(0);
struct DropMarker;
impl Drop for DropMarker { fn drop(&mut self) { TLS_DROPS.fetch_add(1, Ordering::SeqCst); } }
thread_local! { static MARKER: DropMarker = const { DropMarker }; }
use std::time::Duration;
thread_local! { static LOCAL: Cell<u32> = const { Cell::new(7) }; }
unsafe extern "C" {
    fn probe_begin(graphics: i32);
    fn probe_log(s: *const core::ffi::c_char);
    fn probe_poll() -> i32;
    fn probe_end(graphics: i32);
}
fn log(s: &core::ffi::CStr) { unsafe { probe_log(s.as_ptr()); } }
#[unsafe(no_mangle)]
pub extern "C" fn rust_probe_main() -> i32 {
    unsafe { probe_begin(0); }
    std::panic::set_hook(Box::new(|info| {
        let msg = std::ffi::CString::new(format!("FAIL Rust std: {info}")).unwrap();
        log(&msg);
    }));
    log(c"BEGIN Rust std: ABI assertions passed at compile time");
    assert_eq!(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }, 4096);
    assert_eq!(unsafe { libc::sysconf(-1) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EINVAL));
    let values: Vec<u32> = (0..1024).collect();
    assert_eq!(values.iter().sum::<u32>(), 523776);
    drop(values);
    let mut map = std::collections::HashMap::new();
    map.insert("page_size", 4096u32);
    map.insert("horizon_switch", 1u32);
    assert_eq!(map.get("page_size"), Some(&4096));
    assert_eq!(map.get("horizon_switch"), Some(&1));
    drop(map);
    LOCAL.set(19);
    let shared = Arc::new((Mutex::new(false), Condvar::new()));
    let other = shared.clone();
    let worker = std::thread::spawn(move || {
        MARKER.with(|_| {});
        assert_eq!(LOCAL.get(), 7);
        LOCAL.set(37);
        let (lock, cv) = &*other;
        *lock.lock().unwrap() = true;
        cv.notify_one();
        assert_eq!(LOCAL.get(), 37);
    });
    let (lock, cv) = &*shared;
    let done = lock.lock().unwrap();
    let (done, timeout) = cv.wait_timeout_while(done, Duration::from_secs(5), |done| !*done).unwrap();
    assert!(*done && !timeout.timed_out(), "worker timed out");
    drop(done);
    worker.join().expect("std::thread join");
    assert_eq!(LOCAL.get(), 19);
    assert_eq!(TLS_DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(Arc::strong_count(&shared), 1);
    log(c"PASS Rust std: thread, Mutex, Condvar, TLS and join");
    let path = "sdmc:/switch/horizon-tests/rust-std-roundtrip.tmp";
    std::fs::write(path, [0, 127, 128, 255]).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), [0, 127, 128, 255]);
    std::fs::remove_file(path).unwrap();
    log(c"PASS Rust std: file write/read/remove");
    log(c"RESULT=0; press PLUS to exit");
    while unsafe { probe_poll() } != 0 {}
    unsafe { probe_end(0); }
    0
}
