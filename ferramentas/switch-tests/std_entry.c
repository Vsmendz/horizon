#include <switch.h>
#include <stdio.h>
#include <sys/stat.h>
static PadState pad;
static FILE *report;
static int using_graphics;
void probe_log(const char *s) { fprintf(stderr, "%s\n", s); if (!using_graphics) puts(s); }
void probe_begin(int graphics) {
    using_graphics = graphics;
    mkdir("sdmc:/switch/horizon-tests", 0777);
    report = freopen(graphics ? "sdmc:/switch/horizon-tests/test2b.log" : "sdmc:/switch/horizon-tests/test2c_rust_std.log", "w", stderr);
    if (report) setvbuf(stderr, NULL, _IONBF, 0);
    if (!graphics) consoleInit(NULL);
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    padInitializeDefault(&pad);
    fprintf(stderr, "BEGIN Rust probe graphics=%d\n", graphics);
}
int probe_poll(void) {
    if (!appletMainLoop()) return 0;
    padUpdate(&pad);
    if (padGetButtonsDown(&pad) & HidNpadButton_Plus) return 0;
    if (!using_graphics) consoleUpdate(NULL);
    return 1;
}
void probe_end(int graphics) {
    fprintf(stderr, "END Rust probe\n");
    if (!graphics) consoleExit(NULL);
}
extern int rust_probe_main(void);
int main(void) {
    int result = rust_probe_main();
    fprintf(stderr, "RETURN %d\n", result);
    return result;
}

#include <unistd.h>
#include <errno.h>
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
