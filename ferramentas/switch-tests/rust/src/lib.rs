#![no_std]
extern crate alloc;
use alloc::vec::Vec;
use core::{alloc::{GlobalAlloc, Layout}, ffi::{c_char, c_void}, panic::PanicInfo};
unsafe extern "C" {
    fn memalign(align: usize, size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn abort() -> !;
    fn probe_log(s: *const c_char) -> i32;
    fn probe_begin(graphics: i32);
    fn probe_poll() -> i32;
    fn probe_end(graphics: i32);
    #[cfg(feature="egl")]
    fn nwindowGetDefault() -> *mut c_void;
}
struct Newlib;
unsafe impl GlobalAlloc for Newlib {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { memalign(layout.align().max(8), layout.size().max(1)).cast() }
    }
    unsafe fn dealloc(&self, p: *mut u8, _: Layout) { unsafe { free(p.cast()) } }
}
#[global_allocator]
static ALLOC: Newlib = Newlib;
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { probe_log(c"FAIL: Rust panic".as_ptr()); abort() }
}
#[unsafe(no_mangle)]
pub extern "C" fn rust_probe_main() -> i32 {
    unsafe { probe_begin(i32::from(cfg!(feature="egl"))) };
    let values: Vec<u32> = (0..1024).collect();
    let sum: u32 = values.iter().copied().sum();
    if sum != 523776 { unsafe { abort() } }
    drop(values);
    unsafe { probe_log(c"PASS: Rust entry, Vec allocation, checksum and deallocation".as_ptr()); }
    #[cfg(feature="egl")]
    let result = unsafe { graphics::run() };
    #[cfg(not(feature="egl"))]
    let result = {
        unsafe { probe_log(c"Test 2A: Rust/newlib allocation OK. Press PLUS to exit.".as_ptr()); }
        while unsafe { probe_poll() } != 0 {}
        0
    };
    unsafe { probe_end(i32::from(cfg!(feature="egl"))) };
    result
}
#[cfg(feature="egl")]
mod graphics {
    use super::*;
    type Handle = *mut c_void;
    unsafe extern "C" {
        fn eglGetDisplay(native: Handle) -> Handle;
        fn eglInitialize(display: Handle, major: *mut i32, minor: *mut i32) -> u32;
        fn eglBindAPI(api: u32) -> u32;
        fn eglChooseConfig(display: Handle, attrs: *const i32, config: *mut Handle, size: i32, count: *mut i32) -> u32;
        fn eglCreateWindowSurface(display: Handle, config: Handle, window: Handle, attrs: *const i32) -> Handle;
        fn eglCreateContext(display: Handle, config: Handle, share: Handle, attrs: *const i32) -> Handle;
        fn eglMakeCurrent(display: Handle, draw: Handle, read: Handle, context: Handle) -> u32;
        fn eglGetProcAddress(name: *const c_char) -> *const c_void;
        fn eglSwapBuffers(display: Handle, surface: Handle) -> u32;
        fn eglDestroyContext(display: Handle, context: Handle) -> u32;
        fn eglDestroySurface(display: Handle, surface: Handle) -> u32;
        fn eglTerminate(display: Handle) -> u32;
    }
    pub unsafe fn run() -> i32 {
        unsafe {
            let null = core::ptr::null_mut();
            let display = eglGetDisplay(null);
            if display.is_null() || eglInitialize(display, null.cast(), null.cast()) == 0 { return 11; }
            let mut surface = null;
            let mut context = null;
            let result = (|| {
                if eglBindAPI(0x30A0) == 0 { return 12; } // EGL_OPENGL_ES_API
                let attrs = [0x3040, 0x0040, 0x3033, 4, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, 0x3038];
                let mut config = null;
                let mut count = 0;
                if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count) == 0 || count != 1 { return 13; }
                surface = eglCreateWindowSurface(display, config, nwindowGetDefault(), core::ptr::null());
                if surface.is_null() { return 14; }
                context = eglCreateContext(display, config, null, [0x3098, 3, 0x3038].as_ptr());
                if context.is_null() { return 15; }
                if eglMakeCurrent(display, surface, surface, context) == 0 { return 16; }
                let clear_color = eglGetProcAddress(c"glClearColor".as_ptr());
                let clear = eglGetProcAddress(c"glClear".as_ptr());
                if clear_color.is_null() || clear.is_null() { return 17; }
                let clear_color: unsafe extern "C" fn(f32,f32,f32,f32) = core::mem::transmute(clear_color);
                let clear: unsafe extern "C" fn(u32) = core::mem::transmute(clear);
                probe_log(c"PASS: Rust EGL/GLES context and GL entry points; cyan screen expected".as_ptr());
                while probe_poll() != 0 {
                    clear_color(0.0,0.6,0.8,1.0);
                    clear(0x4000);
                    if eglSwapBuffers(display, surface) == 0 { return 18; }
                }
                0
            })();
            eglMakeCurrent(display, null, null, null);
            if !context.is_null() { eglDestroyContext(display, context); }
            if !surface.is_null() { eglDestroySurface(display, surface); }
            eglTerminate(display);
            result
        }
    }
}
