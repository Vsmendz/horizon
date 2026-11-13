/* The SDK owns Framebuffer's layout. Keep it out of the Rust FFI. */
#include <switch.h>

static Framebuffer splash;

u32 enhanced_splash_open(void) {
    NWindow *window = nwindowGetDefault();
    if (!window || !nwindowIsValid(window)) return MAKERESULT(Module_Libnx, LibnxError_BadInput);
    Result rc = framebufferCreate(&splash, window, 1280, 720, PIXEL_FORMAT_RGBA_8888, 2);
    if (R_FAILED(rc)) return rc;
    rc = framebufferMakeLinear(&splash);
    if (R_FAILED(rc)) framebufferClose(&splash);
    return rc;
}

void *enhanced_splash_begin(u32 *stride) { return framebufferBegin(&splash, stride); }
void enhanced_splash_end(void) { framebufferEnd(&splash); }
void enhanced_splash_close(void) { framebufferClose(&splash); }
u64 enhanced_boot_tick(void) { return armGetSystemTick(); }
u64 enhanced_boot_ms(u64 start) { return armTicksToNs(armGetSystemTick() - start) / 1000000; }
