#include "nxlink_log.h"

#ifdef __SWITCH__
#include <switch.h>
#else
#include <semaphore.h>
#endif
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#ifndef NXLINK_CLIENT_PORT
#define NXLINK_CLIENT_PORT 28771
#endif

enum { QUEUE_SIZE = 128, LINE_SIZE = 512 };
static char queue[QUEUE_SIZE][LINE_SIZE];
// Only the application thread publishes records; the worker consumes them.
static atomic_uint head, tail;
#ifdef __SWITCH__
static LEvent ready;
#else
static sem_t ready;
#endif
static pthread_t worker;
static atomic_bool accepting;
static atomic_bool stopping;
static atomic_uint dropped;
static bool started;
static uint32_t address;
static const char *failure;

static void wake_worker(void) {
#ifdef __SWITCH__
    leventSignal(&ready);
#else
    sem_post(&ready);
#endif
}

static void wait_for_record(void) {
#ifdef __SWITCH__
    leventWait(&ready, UINT64_MAX);
#else
    while (sem_wait(&ready) == -1 && errno == EINTR) {}
#endif
}

static uint64_t monotonic_ms(void) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (uint64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

static bool writable(int fd, int timeout) {
    struct pollfd f = { .fd = fd, .events = POLLOUT };
    return poll(&f, 1, timeout) > 0 && (f.revents & POLLOUT)
        && !(f.revents & (POLLERR | POLLHUP | POLLNVAL));
}

// The nxlink callback is a raw TCP stream. Own the socket instead of redirecting
// stdio, so application logging never waits on Wi-Fi and connection has a deadline.
static int connect_receiver(void) {
    struct sockaddr_in peer = {
#ifdef __SWITCH__
        .sin_len = sizeof(peer),
#endif
        .sin_family = AF_INET,
        .sin_port = htons(NXLINK_CLIENT_PORT),
        .sin_addr = { .s_addr = address },
    };
    const uint64_t deadline = monotonic_ms() + 2500;
    do {
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) return -1;
        int flags = fcntl(fd, F_GETFL, 0);
        if (flags >= 0 && fcntl(fd, F_SETFL, flags | O_NONBLOCK) == 0) {
            if (connect(fd, (struct sockaddr *)&peer, sizeof(peer)) == 0) return fd;
            if (errno == EINPROGRESS && writable(fd, 120)) {
                int error = 0;
                socklen_t size = sizeof(error);
                if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &size) == 0 && error == 0) {
                    return fd;
                }
            }
        }
        close(fd);
        struct timespec pause = { .tv_nsec = 50000000 };
        nanosleep(&pause, NULL);
    } while (monotonic_ms() < deadline);
    return -1;
}

static bool send_line(int fd, const char *line) {
    const uint64_t deadline = monotonic_ms() + 150;
    size_t left = strlen(line);
    while (left) {
        if (monotonic_ms() >= deadline) return false;
        ssize_t n = send(fd, line, left, MSG_NOSIGNAL);
        if (n > 0) {
            line += n;
            left -= (size_t)n;
        } else if (n == 0 || (errno != EINTR
                   && ((errno != EAGAIN && errno != EWOULDBLOCK) || !writable(fd, 100)))) {
            return false;
        }
    }
    return true;
}

static void *send_logs(void *unused) {
    (void)unused;
#ifdef __SWITCH__
    if (R_FAILED(socketInitializeDefault())) {
        failure = "socket initialization failed";
        atomic_store(&accepting, false);
        return NULL;
    }
#endif
    int fd = connect_receiver();
    if (fd < 0) {
        failure = "nxlink callback was unreachable";
    } else {
        uint64_t drain_deadline = 0;
        for (;;) {
            const bool stop = atomic_load(&stopping);
            unsigned read = atomic_load_explicit(&head, memory_order_relaxed);
            unsigned end = atomic_load_explicit(&tail, memory_order_acquire);
            if (stop && !drain_deadline) drain_deadline = monotonic_ms() + 500;
            if (read == end) {
                if (stop) break;
                wait_for_record();
                continue;
            }
            if (drain_deadline && monotonic_ms() >= drain_deadline) {
                atomic_fetch_add(&dropped, end - read);
                break;
            }
            if (!send_line(fd, queue[read % QUEUE_SIZE])) {
                failure = "nxlink callback disconnected or stalled";
                break;
            }
            atomic_store_explicit(&head, read + 1, memory_order_release);
        }
        if (!failure && atomic_load(&dropped)) {
            char line[64];
            snprintf(line, sizeof(line), "NXLINK_DROPPED=%u\n", atomic_load(&dropped));
            if (!send_line(fd, line)) failure = "failed to report dropped network records";
        }
        shutdown(fd, SHUT_WR);
        close(fd);
    }
    atomic_store(&accepting, false);
#ifdef __SWITCH__
    socketExit();
#endif
    return NULL;
}

void nxlog_start(uint32_t host) {
    if (!host) return;
    address = host;
#ifdef __SWITCH__
    leventInit(&ready, false, true);
#else
    if (sem_init(&ready, 0, 0) != 0) {
        failure = "could not initialize callback wake event";
        return;
    }
#endif
    atomic_store(&accepting, true);
    if (pthread_create(&worker, NULL, send_logs, NULL) != 0) {
        atomic_store(&accepting, false);
        failure = "could not start nxlink log thread";
#ifndef __SWITCH__
        sem_destroy(&ready);
#endif
        return;
    }
    started = true;
}

void nxlog_push(const char *line) {
    if (!atomic_load(&accepting)) return;
    size_t size = strlen(line);
    unsigned next = atomic_load_explicit(&tail, memory_order_relaxed);
    unsigned read = atomic_load_explicit(&head, memory_order_acquire);
    if (size > LINE_SIZE - 2 || next - read >= QUEUE_SIZE) {
        atomic_fetch_add(&dropped, 1);
        return;
    }
    char *entry = queue[next % QUEUE_SIZE];
    memcpy(entry, line, size);
    entry[size] = '\n';
    entry[size + 1] = '\0';
    atomic_store_explicit(&tail, next + 1, memory_order_release);
    wake_worker();
}

NxLogResult nxlog_finish(void) {
    if (started) {
        atomic_store(&stopping, true);
        wake_worker();
        pthread_join(worker, NULL);
#ifndef __SWITCH__
        sem_destroy(&ready);
#endif
    }
    return (NxLogResult){ failure, atomic_load(&dropped) };
}
