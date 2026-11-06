#include <switch.h>
#include <stdio.h>
#include <stdint.h>
#include <sys/stat.h>
#include <unistd.h>
#include <errno.h>

static PadState s_pad;
static FILE *s_report;

void probe_log(const char *s) {
    if (s_report) {
        fprintf(stderr, "%s\n", s);
    }
}

void probe_begin(void) {
    mkdir("sdmc:/switch/horizon-tests", 0777);
    s_report = freopen("sdmc:/switch/horizon-tests/test3_horizon_integration.log", "w", stderr);
    if (s_report) {
        setvbuf(stderr, NULL, _IONBF, 0);
    }
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    padInitializeDefault(&s_pad);
    fprintf(stderr, "BEGIN test3_horizon_integration\n");
}

void *horizon_switch_default_window(void) {
    return (void *)nwindowGetDefault();
}

int horizon_switch_poll(
    uint64_t *buttons_down_out,
    uint64_t *buttons_held_out,
    int32_t *lx_out,
    int32_t *ly_out,
    int32_t *rx_out,
    int32_t *ry_out
) {
    if (!appletMainLoop()) {
        return 0;
    }
    padUpdate(&s_pad);
    u64 kDown = padGetButtonsDown(&s_pad);
    u64 kHeld = padGetButtons(&s_pad);
    HidAnalogStickState left_stick = padGetStickPos(&s_pad, 0);
    HidAnalogStickState right_stick = padGetStickPos(&s_pad, 1);

    if (buttons_down_out) *buttons_down_out = (uint64_t)kDown;
    if (buttons_held_out) *buttons_held_out = (uint64_t)kHeld;
    if (lx_out) *lx_out = left_stick.x;
    if (ly_out) *ly_out = left_stick.y;
    if (rx_out) *rx_out = right_stick.x;
    if (ry_out) *ry_out = right_stick.y;

    if (kDown & HidNpadButton_Plus) {
        fprintf(stderr, "EVENT Plus button pressed; requesting clean shutdown\n");
        return 0;
    }
    return 1;
}

void probe_end(void) {
    fprintf(stderr, "END test3_horizon_integration\n");
}

extern int rust_horizon_integration_main(void);

int main(void) {
    probe_begin();
    int result = rust_horizon_integration_main();
    fprintf(stderr, "RETURN %d\n", result);
    probe_end();
    return result;
}

// Horizon OS has 4 KiB pages: libnx/switch.ld aligns memory segments to 0x1000;
// libnx/include/switch/kernel/svc.h documents 0x1000 mapping granularity.
// Only the page-size query is implemented. Unsupported names fail explicitly.
long horizon_switch_sysconf(int name) {
    if (name == _SC_PAGESIZE) return 0x1000;
    errno = EINVAL;
    return -1;
}

long sysconf(int name) {
    return horizon_switch_sysconf(name);
}

// Rust std::sys::random on target_os = "horizon" calls libc::getrandom.
// Back it with libnx's OS-seeded ChaCha20 CSPRNG (randomGet in switch/kernel/random.h).
ssize_t getrandom(void *buf, size_t buflen, unsigned int flags) {
    if (buf == NULL && buflen > 0) {
        errno = EFAULT;
        return -1;
    }
    if ((flags & ~0x3U) != 0U) {
        errno = EINVAL;
        return -1;
    }
    if (buflen > 0) {
        randomGet(buf, buflen);
    }
    return (ssize_t)buflen;
}
