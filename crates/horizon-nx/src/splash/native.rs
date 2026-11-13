use super::{HEIGHT, WIDTH, paint};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

unsafe extern "C" {
    fn enhanced_splash_open() -> u32;
    fn enhanced_splash_begin(stride: *mut u32) -> *mut u32;
    fn enhanced_splash_end();
    fn enhanced_splash_close();
    fn enhanced_boot_tick() -> u64;
    fn enhanced_boot_ms(start: u64) -> u64;
}

static START: AtomicU64 = AtomicU64::new(0);
static STATE: Mutex<Option<Option<u64>>> = Mutex::new(None);

pub(crate) fn mark_boot() {
    START.store(unsafe { enhanced_boot_tick() }, Ordering::Relaxed);
}

fn boot_ms() -> u64 {
    unsafe { enhanced_boot_ms(START.load(Ordering::Relaxed)) }
}

pub fn stamp(stage: &str) {
    crate::logging::write_log_line(format_args!(
        "[Enhanced] stage={stage} boot_ms={}",
        boot_ms()
    ));
}

/// Runs on the render thread before resource workers start.
pub fn open() {
    let mut state = STATE.lock().unwrap();
    if state.is_some() {
        return;
    }
    stamp("native_surface_requested");
    let result = unsafe { enhanced_splash_open() };
    if result != 0 {
        crate::logging::write_log_line(format_args!("[Enhanced] framebuffer result={result:#x}"));
        return;
    }
    *state = Some(None);
    drop(state);
    draw("PREPARING SERVICES");
}

/// Advance an indeterminate bar at most ten times per second.
pub fn draw(stage: &str) {
    let mut state = STATE.lock().unwrap();
    let Some(last_frame) = state.as_mut() else {
        return;
    };
    let now = boot_ms();
    if last_frame.is_some_and(|last| now.saturating_sub(last) < 100) {
        return;
    }
    let mut stride_bytes = 0;
    let ptr = unsafe { enhanced_splash_begin(&mut stride_bytes) };
    if ptr.is_null() {
        return;
    }
    let stride = stride_bytes as usize / size_of::<u32>();
    // SAFETY: libnx allocated the linear shadow buffer with this row stride
    // for the 720 rows requested. The mutex owns the open framebuffer.
    if stride >= WIDTH && stride_bytes % 4 == 0 {
        let pixels = unsafe { std::slice::from_raw_parts_mut(ptr, stride * HEIGHT) };
        paint(pixels, stride, stage, now);
    }
    unsafe { enhanced_splash_end() };
    if last_frame.is_none() {
        stamp("first_native_frame_queued");
    }
    *last_frame = Some(now);
}

pub fn active() -> bool {
    STATE.lock().unwrap().is_some()
}

/// Release every registered buffer before Vulkan claims NWindow.
pub fn close() {
    let mut state = STATE.lock().unwrap();
    if state.take().is_some() {
        unsafe { enhanced_splash_close() };
        stamp("native_surface_released");
    }
}
