#include "panel_id.h"

// Steht `pattern` (bits breit) irgendwo in den 40 gelesenen Bits?
static bool contains_bits(uint64_t raw, uint32_t pattern, int bits)
{
    const uint64_t mask = (1ULL << bits) - 1;
    for (int shift = 0; shift + bits <= PANEL_READ_BITS; ++shift) {
        if (((raw >> shift) & mask) == pattern) return true;
    }
    return false;
}

PanelKind panel_classify(uint64_t raw04, uint64_t rawD3)
{
    // Offene oder nicht getriebene Leitung liest nur Einsen (Pull-up) oder
    // nur Nullen — beides passt auf keine der Signaturen.
    if (contains_bits(raw04, 0x8585, 16)) return PANEL_ST7789;
    if (contains_bits(rawD3, 0x9341, 16)) return PANEL_ILI9341;
    if (contains_bits(rawD3, 0x9342, 16)) return PANEL_ILI9342;
    return PANEL_UNKNOWN;
}

bool panel_line_floats(uint64_t pulled_up, uint64_t pulled_down)
{
    const uint64_t all = (1ULL << PANEL_READ_BITS) - 1;
    return pulled_up == all && pulled_down == 0;
}

const char* panel_kind_id(PanelKind kind)
{
    switch (kind) {
        case PANEL_ILI9341: return "ili9341";
        case PANEL_ILI9342: return "ili9342";
        case PANEL_ST7789:  return "st7789";
        case PANEL_NO_REPLY: return "noreply";
        default:            return "unknown";
    }
}
