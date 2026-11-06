#pragma once

#include <stdint.h>

typedef struct {
    const char *error;
    unsigned dropped;
} NxLogResult;

// host is the IPv4 address from the loader, in network byte order.
void nxlog_start(uint32_t host);
void nxlog_push(const char *line);
NxLogResult nxlog_finish(void);
