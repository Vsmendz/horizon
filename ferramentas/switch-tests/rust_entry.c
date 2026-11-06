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
    report = freopen(graphics ? "sdmc:/switch/horizon-tests/test2b.log" : "sdmc:/switch/horizon-tests/test2a.log", "w", stderr);
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
