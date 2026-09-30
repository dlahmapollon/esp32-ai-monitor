#pragma once
// Host-only shim. No device I/O is performed while rendering website examples.
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <algorithm>
using std::min;
using std::max;
inline unsigned long millis() { return 120000; }
struct SiteSerial {
    template<typename... T> void printf(const char *, T...) {}
    void println(const char *) {}
};
inline SiteSerial Serial;
