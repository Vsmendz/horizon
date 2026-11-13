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

void enhanced_touch_init(void) { hidInitializeTouchScreen(); }

bool enhanced_touch_sample(u32 *id, u32 *x, u32 *y) {
    HidTouchScreenState state;
    if (!hidGetTouchScreenStates(&state, 1) || state.count < 1) return false;
    *id = state.touches[0].finger_id;
    *x = state.touches[0].x;
    *y = state.touches[0].y;
    return true;
}

bool enhanced_ui_focused(void) { return appletGetFocusState() == AppletFocusState_InFocus; }
