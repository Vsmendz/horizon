#include <switch.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <time.h>
#include <sys/stat.h>

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static pthread_key_t key;
static _Thread_local int local_value = 7;
static int ready, release_worker, worker_ok, destructed;
static int main_token, worker_token;
static FILE *logfile;
static void report(const char *text) {
    puts(text);
    if (logfile) { fprintf(logfile, "%s\n", text); fflush(logfile); }
}
static void destructor(void *p) { if (p == &worker_token) destructed++; }
static struct timespec deadline(void) {
    struct timespec t;
    clock_gettime(CLOCK_REALTIME, &t);
    t.tv_sec += 5;
    return t;
}
static void *worker(void *unused) {
    (void)unused;
    int ok = local_value == 7 && pthread_getspecific(key) == NULL;
    local_value = 37;
    ok = ok && pthread_setspecific(key, &worker_token) == 0;
    if (pthread_mutex_lock(&mutex)) return NULL;
    ready = 1;
    pthread_cond_broadcast(&cond);
    struct timespec until = deadline();
    int status = 0;
    while (!release_worker && !status) status = pthread_cond_timedwait(&cond, &mutex, &until);
    worker_ok = ok && status == 0 && release_worker && local_value == 37 &&
                pthread_getspecific(key) == &worker_token;
    pthread_mutex_unlock(&mutex);
    return NULL;
}
static int run_test(void) {
    pthread_t thread;
    local_value = 19;
    if (pthread_key_create(&key, destructor)) return 1;
    if (pthread_setspecific(key, &main_token)) return 2;
    if (pthread_create(&thread, NULL, worker, NULL)) return 3;
    if (pthread_mutex_lock(&mutex)) return 4;
    struct timespec until = deadline();
    int status = 0;
    while (!ready && !status) status = pthread_cond_timedwait(&cond, &mutex, &until);
    release_worker = 1;
    pthread_cond_broadcast(&cond);
    pthread_mutex_unlock(&mutex);
    if (pthread_join(thread, NULL)) return 5;
    if (status || !worker_ok || destructed != 1 || local_value != 19 ||
        pthread_getspecific(key) != &main_token) return 6;
    pthread_key_delete(key);
    pthread_cond_destroy(&cond);
    pthread_mutex_destroy(&mutex);
    report("PASS C: pthread, mutex, condvar, compiler TLS and pthread TLS destructor");
    const char *path = "sdmc:/switch/horizon-tests/posix-roundtrip.tmp";
    FILE *f = fopen(path, "wb");
    if (!f) return 7;
    const unsigned char expected[] = {0, 1, 127, 128, 255, 42};
    int ok = fwrite(expected, 1, sizeof(expected), f) == sizeof(expected);
    ok = fflush(f) == 0 && ok;
    ok = fclose(f) == 0 && ok;
    if (!ok) return 8;
    f = fopen(path, "rb");
    if (!f) return 9;
    unsigned char actual[sizeof(expected)];
    ok = fread(actual, 1, sizeof(actual), f) == sizeof(actual);
    ok = fgetc(f) == EOF && !ferror(f) && ok;
    ok = fclose(f) == 0 && ok;
    if (!ok || memcmp(expected, actual, sizeof(expected))) return 10;
    if (remove(path)) return 11;
    report("PASS C: newlib SD file binary roundtrip and removal");
    return 0;
}
int main(void) {
    consoleInit(NULL);
    mkdir("sdmc:/switch/horizon-tests", 0777);
    logfile = fopen("sdmc:/switch/horizon-tests/test2c_c.log", "w");
    report("Test 2C C/newlib only: this does NOT validate Rust std.");
    int result = run_test();
    char message[128];
    snprintf(message, sizeof(message), "RESULT=%d (zero=PASS); press PLUS to exit", result);
    report(message);
    PadState pad;
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    padInitializeDefault(&pad);
    while (appletMainLoop()) {
        padUpdate(&pad);
        if (padGetButtonsDown(&pad) & HidNpadButton_Plus) break;
        consoleUpdate(NULL);
    }
    if (logfile) fclose(logfile);
    consoleExit(NULL);
    return result;
}
