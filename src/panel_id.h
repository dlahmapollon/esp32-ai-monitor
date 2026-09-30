#ifndef PANEL_ID_H
#define PANEL_ID_H

#include <stdint.h>

// ============================================================
// Panel-Erkennung fuer die CYDs
// ------------------------------------------------------------
// Unter „ESP32-2432S028" laufen Boards mit ILI9341, ST7789 und ILI9342. Der
// ESP32 ist bei allen derselbe, die Firmware-Variante haengt also allein am
// Display-Controller. Den verraten dessen ID-Register:
//
//   RDDID (0x04)  ST7789:  85 85 52
//   RDID4 (0xD3)  ILI9341: 00 93 41, ILI9342: 00 93 42
//
// Wie viele Dummy-Takte vor den Daten kommen (keiner, ein Bit, ein Byte),
// schwankt zwischen Controllern und Datenblaettern. Deshalb liest
// board_cyd.cpp 40 Bit am Stueck, und hier wird die Signatur an jeder
// Bitposition gesucht. Reine Logik ohne Hardware, nativ getestet
// (tests/panel_id_native.cpp).
// ============================================================

enum PanelKind {
    PANEL_UNKNOWN = 0,
    PANEL_ILI9341,
    PANEL_ILI9342,
    PANEL_ST7789,
    // Keine ID lesbar, MISO haengt offen: das Panel hat keine Rueckleitung.
    // Beim ST7789-CYD so gemessen; ein Beweis fuer ST7789 ist es nicht.
    PANEL_NO_REPLY,
};

// Anzahl Bits, die board_cyd.cpp nach dem Befehl einliest (MSB zuerst).
static const int PANEL_READ_BITS = 40;

// `raw04` und `rawD3`: die 40 Bit nach Befehl 0x04 bzw. 0xD3.
PanelKind panel_classify(uint64_t raw04, uint64_t rawD3);

// Haengt die Leitung offen? `pulled_up` und `pulled_down`: dieselbe Lesung
// einmal mit Pull-up, einmal mit Pull-down. Ein Panel, das die Leitung
// treibt, liefert mindestens eine Null bzw. Eins gegen den Widerstand.
bool panel_line_floats(uint64_t pulled_up, uint64_t pulled_down);

// Kennung wie im get_info-Feld `panel` ("ili9341", "st7789", ...).
const char* panel_kind_id(PanelKind kind);

#endif // PANEL_ID_H
