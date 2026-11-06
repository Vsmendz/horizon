#include <switch.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <errno.h>
#ifdef HORIZON_VANILLA_CLIENT
#include <malloc.h>
#endif
#ifdef HORIZON_NXLINK_LOG
#include <netinet/in.h>
#include "nxlink_log.h"
#endif

static PadState s_pad;
static FILE *s_report;
static int s_romfs_mounted = 0;
static char s_log_error[256];
#ifdef HORIZON_VANILLA_CLIENT
static int s_loading;
#endif

#ifndef HORIZON_CLIENT_TEST_NAME
#define HORIZON_CLIENT_TEST_NAME "test5_horizon_client"
#endif
#define LOG_DIR "sdmc:/switch/horizon-tests"
#define LOG_PATH LOG_DIR "/" HORIZON_CLIENT_TEST_NAME ".log"

static void report_log_error(const char *action, int error) {
    if (s_log_error[0]) return;
    snprintf(s_log_error, sizeof(s_log_error), "%s: %s (errno=%d)",
             action, strerror(error), error);
    fprintf(stderr, "%s\n%s\n", LOG_PATH, s_log_error);
    fflush(stderr);
}

static int ensure_directory(const char *path) {
    if (mkdir(path, 0777) == 0) return 0;
    if (errno != EEXIST) return -1;
    struct stat info;
    if (stat(path, &info) != 0) return -1;
    if (S_ISDIR(info.st_mode)) return 0;
    errno = ENOTDIR;
    return -1;
}

void probe_log(const char *s) {
#ifdef HORIZON_NXLINK_LOG
    nxlog_push(s);
#endif
#ifdef HORIZON_WORLDGEN_PROBE
    puts(s);
    consoleUpdate(NULL);
#endif
#ifdef HORIZON_VANILLA_CLIENT
    if (s_loading) {
        puts(s);
        consoleUpdate(NULL);
    }
#endif
    if (!s_report || s_log_error[0]) {
        fprintf(stderr, "%s\n", s);
        fflush(stderr);
    } else if (fprintf(s_report, "%s\n", s) < 0 || fflush(s_report) == EOF) {
        report_log_error("write/flush log", errno);
    }
}

#ifdef HORIZON_VANILLA_CLIENT
void horizon_loading_begin(void) {
    consoleInit(NULL);
    s_loading = 1;
    puts("Horizon 26.3 - loading world resources and terrain...");
    consoleUpdate(NULL);
}

int horizon_loading_poll(void) {
    if (!appletMainLoop()) return 0;
    padUpdate(&s_pad);
    consoleUpdate(NULL);
    return (padGetButtonsDown(&s_pad) & HidNpadButton_Plus) == 0;
}

void horizon_loading_end(void) {
    s_loading = 0;
    consoleExit(NULL);
}

void horizon_memory_report(void) {
    extern char *fake_heap_start, *fake_heap_end;
    u64 mapped = 0, limit = 0;
    Result used_result = svcGetInfo(&mapped, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
    Result limit_result = svcGetInfo(&limit, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
    struct mallinfo stats = mallinfo();
    char msg[256];
    snprintf(msg, sizeof(msg),
             "MEMORY kernel_used_bytes=%llu kernel_limit_bytes=%llu svc_status=0x%x/0x%x libnx_heap_bytes=%zu allocator_in_use_bytes=%zu allocator_arena_bytes=%zu",
             (unsigned long long)mapped, (unsigned long long)limit, used_result, limit_result,
             (size_t)((uintptr_t)fake_heap_end - (uintptr_t)fake_heap_start), stats.uordblks, stats.arena);
    probe_log(msg);
}
#endif

void probe_begin(void) {
    if (!fsdevGetDeviceFileSystem("sdmc")) {
        Result rc = fsdevMountSdmc();
        if (R_FAILED(rc)) {
            snprintf(s_log_error, sizeof(s_log_error), "fsdevMountSdmc failed: 0x%x", rc);
        }
    }
    if (!s_log_error[0]) {
        if (ensure_directory("sdmc:/switch") != 0 || ensure_directory(LOG_DIR) != 0) {
            report_log_error("create log directory", errno);
        } else {
            s_report = fopen(LOG_PATH, "w");
            if (!s_report) report_log_error("open log", errno);
        }
    }
    probe_log("BEGIN " HORIZON_CLIENT_TEST_NAME);
#ifdef HORIZON_NXLINK_LOG
    if (!__nxlink_host.s_addr) probe_log("INFO nxlink callback inactive: no host from loader");
#endif
#ifndef HORIZON_WORLDGEN_PROBE
    const char *save_dirs[] = {
        "sdmc:/switch/horizon", "sdmc:/switch/horizon/saves",
    };
    for (size_t i = 0; i < sizeof(save_dirs) / sizeof(save_dirs[0]); i++) {
        if (ensure_directory(save_dirs[i]) != 0) {
            int error = errno;
            char message[256];
            snprintf(message, sizeof(message), "WARN mkdir %s: %s", save_dirs[i], strerror(error));
            probe_log(message);
            break;
        }
    }
#endif
    Result rc = romfsInit();
    if (R_SUCCEEDED(rc)) {
        s_romfs_mounted = 1;
    } else {
        char message[128];
        snprintf(message, sizeof(message), "WARN romfsInit returned 0x%x", rc);
        probe_log(message);
    }
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    padInitializeDefault(&s_pad);
}

int horizon_switch_romfs_mounted(void) {
    return s_romfs_mounted;
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
        probe_log("INFO Plus button pressed; requesting clean shutdown");
        return 0;
    }
    return 1;
}

void probe_end(void) {
    if (s_romfs_mounted) {
        romfsExit();
        s_romfs_mounted = 0;
    }
    probe_log("END " HORIZON_CLIENT_TEST_NAME);
    if (s_report && fclose(s_report) == EOF) report_log_error("close log", errno);
    s_report = NULL;
}

#ifndef HORIZON_WORLDGEN_PROBE
static void show_log_error(void) {
    if (!s_log_error[0]) return;
    consoleInit(NULL);
    printf("Horizon log error\n%s\n%s\n\nPress PLUS to return.\n", LOG_PATH, s_log_error);
    while (appletMainLoop()) {
        consoleUpdate(NULL);
        padUpdate(&s_pad);
        if (padGetButtonsDown(&s_pad) & HidNpadButton_Plus) break;
    }
    consoleExit(NULL);
}
#endif

#ifdef HORIZON_WORLDGEN_PROBE
extern int rust_horizon_worldgen_main(void);

void horizon_worldgen_refresh(void) { consoleUpdate(NULL); }

uint64_t horizon_worldgen_memory(void) {
    u64 used = 0;
    svcGetInfo(&used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
    return used;
}
#else
extern int rust_horizon_client_main(void);
#endif

int main(void) {
#ifdef HORIZON_NXLINK_LOG
    nxlog_start(__nxlink_host.s_addr);
#endif
#ifdef HORIZON_WORLDGEN_PROBE
    consoleInit(NULL);
#endif
    probe_begin();
#ifdef HORIZON_WORLDGEN_PROBE
    int result = rust_horizon_worldgen_main();
#else
    int result = rust_horizon_client_main();
#endif
    char status[64];
    snprintf(status, sizeof(status), "RESULT=%d\nRETURN %d", result, result);
    probe_log(status);
#ifdef HORIZON_NXLINK_LOG
    // Send the final marker before draining and closing the callback.
    nxlog_push("END " HORIZON_CLIENT_TEST_NAME);
    NxLogResult capture = nxlog_finish();
    if (capture.error || capture.dropped) {
        char warning[160];
        snprintf(warning, sizeof(warning), "WARN nxlink capture incomplete: %s (dropped=%u)",
                 capture.error ? capture.error : "queue capacity exceeded", capture.dropped);
        probe_log(warning);
    }
#endif
    probe_end();
#ifdef HORIZON_WORLDGEN_PROBE
    if (s_log_error[0]) printf("Log error: %s\n%s\n", LOG_PATH, s_log_error);
    puts("Press PLUS to return.");
    while (appletMainLoop()) {
        padUpdate(&s_pad);
        if (padGetButtonsDown(&s_pad) & HidNpadButton_Plus) break;
        consoleUpdate(NULL);
    }
    consoleExit(NULL);
    if (s_log_error[0] && result == 0) return 1;
#else
    show_log_error();
#endif
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
