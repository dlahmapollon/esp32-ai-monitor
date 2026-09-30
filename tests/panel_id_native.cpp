#include "panel_id.h"

#include <assert.h>
#include <stdio.h>
#include <initializer_list>

// 24-Bit-ID in einen 40-Bit-Rahmen legen, davor `dummy` Takte.
static uint64_t frame(uint32_t id24, int dummy, bool pullup)
{
    uint64_t raw = (uint64_t)id24 << (PANEL_READ_BITS - 24 - dummy);
    if (pullup) raw |= (1ULL << (PANEL_READ_BITS - 24 - dummy)) - 1;  // Rest haengt auf 1
    return raw;
}

int main()
{
    const uint64_t ones  = (1ULL << PANEL_READ_BITS) - 1;
    const uint64_t zeros = 0;

    for (int dummy : {0, 1, 8}) {
        for (bool pullup : {false, true}) {
            assert(panel_classify(frame(0x858552, dummy, pullup), ones) == PANEL_ST7789);
            assert(panel_classify(zeros, frame(0x009341, dummy, pullup)) == PANEL_ILI9341);
            assert(panel_classify(ones, frame(0x009342, dummy, pullup)) == PANEL_ILI9342);
        }
    }
    // D9-Umweg: drei Einzelbytes, board_cyd.cpp legt sie oben in den Rahmen.
    assert(panel_classify(zeros, (uint64_t)0x009341 << 16) == PANEL_ILI9341);

    // Nicht angeschlossene Leitung: nichts erkannt.
    assert(panel_classify(ones, ones) == PANEL_UNKNOWN);
    assert(panel_classify(zeros, zeros) == PANEL_UNKNOWN);
    // ILI9341 meldet in 0x04 oft nur Nullen, das darf nicht stoeren.
    assert(panel_classify(zeros, frame(0x009341, 1, false)) == PANEL_ILI9341);

    // ST7789-CYD (Hardware-Test): offene Leitung folgt dem Widerstand.
    assert(panel_line_floats(ones, zeros));
    // ILI9341-CYD: 0x04 liefert zuerst Nullen gegen den Pull-up.
    assert(!panel_line_floats(0x0000007fffULL, zeros));
    assert(!panel_line_floats(ones, 0x0000007fffULL));

    assert(panel_kind_id(PANEL_ST7789)[0] == 's');
    assert(panel_kind_id(PANEL_NO_REPLY)[0] == 'n');
    puts("panel_id: ok");
    return 0;
}
