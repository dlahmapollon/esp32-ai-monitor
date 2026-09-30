/**
 * Board-Umsetzung: CYD (ESP32-2432S028 / -S028R)
 *
 * SPI-Panel (ILI9341 oder ST7789, per Build-Flag) ueber TFT_eSPI, XPT2046-Touch
 * auf eigenem HSPI-Bus ueber touch_input.cpp (TFT_eSPI kann ihn nicht lesen). Der Code stammt unveraendert aus main.cpp v2.17.0; die
 * muehsam ermittelten Panel-Einstellungen (BGR, keine Inversion, Backlight
 * ueber eigenen LEDC-Channel) bleiben genau so, wie sie waren.
 */

#if defined(BOARD_CYD)

#include "board.h"
#include "config.h"
#include "panel_id.h"
#include "touch_input.h"

#include <Arduino.h>
#include <SPI.h>
#include <TFT_eSPI.h>

static TFT_eSPI tft = TFT_eSPI();
// Der Touch-Treiber rechnet die Koordinaten selbst passend zur Drehung um.
static uint8_t touch_orientation = ORIENTATION_PORTRAIT;

// LVGL-Puffer: ein Streifen ueber 10 Zeilen, doppelt. Klein genug fuer den
// internen Speicher, gross genug, dass LVGL nicht in Winzstuecken flusht.
static const uint32_t LV_BUF_PX = DISPLAY_SHORT_SIDE * 10;
static lv_color_t lv_buf1[LV_BUF_PX];
static lv_color_t lv_buf2[LV_BUF_PX];

// ------------------------------------------------------------
// Panel-Erkennung (seit FW 2.23.0)
//
// Laeuft vor tft.init() per Bit-Bang auf den Display-Pins, langsam und
// unabhaengig von TFT_eSPI (das kennt nur den per Build-Flag gewaehlten
// Controller). tft.init() setzt das Panel danach per Software-Reset zurueck,
// die Leseversuche hinterlassen also nichts.
//
// Gelesen wird zuerst ueber MISO (GPIO12, beim Original-CYD belegt). Bleibt
// das stumm, ueber die Datenleitung MOSI (GPIO13): Panels im 3-Draht-Modus
// antworten dort, und bei der ST7789-Variante ist nicht belegt, dass MISO
// ueberhaupt angeschlossen ist. Zuletzt der ILI9341-Umweg ueber 0xD9, den
// auch TFT_eSPI::readcommand8() nimmt.
//
// Beim ST7789-CYD antwortet das Panel auf keiner Leitung. Dann meldet die
// Firmware „noreply", und die Mac-App entscheidet, was daraus folgt.
// ------------------------------------------------------------

static PanelKind panel_kind = PANEL_UNKNOWN;
static char panel_raw[64] = "";

static void bb_clock_out(uint8_t value)
{
    for (int bit = 7; bit >= 0; --bit) {
        digitalWrite(PIN_TFT_MOSI, (value >> bit) & 1);
        delayMicroseconds(1);
        digitalWrite(PIN_TFT_SCLK, HIGH);
        delayMicroseconds(1);
        digitalWrite(PIN_TFT_SCLK, LOW);
    }
}

// Das Panel legt Daten mit der fallenden Flanke an, gelesen wird nach der
// steigenden.
static uint64_t bb_clock_in(int pin, int bits)
{
    uint64_t value = 0;
    for (int i = 0; i < bits; ++i) {
        digitalWrite(PIN_TFT_SCLK, HIGH);
        delayMicroseconds(1);
        value = (value << 1) | (digitalRead(pin) ? 1 : 0);
        digitalWrite(PIN_TFT_SCLK, LOW);
        delayMicroseconds(1);
    }
    return value;
}

static void bb_command(uint8_t cmd)
{
    digitalWrite(PIN_TFT_DC, LOW);
    bb_clock_out(cmd);
    digitalWrite(PIN_TFT_DC, HIGH);
}

static uint64_t bb_read(uint8_t cmd, int pin, int bits)
{
    digitalWrite(PIN_TFT_CS, LOW);
    bb_command(cmd);
    if (pin == PIN_TFT_MOSI) pinMode(PIN_TFT_MOSI, INPUT_PULLUP);
    uint64_t value = bb_clock_in(pin, bits);
    digitalWrite(PIN_TFT_CS, HIGH);
    if (pin == PIN_TFT_MOSI) pinMode(PIN_TFT_MOSI, OUTPUT);
    delayMicroseconds(5);
    return value;
}

// ILI9341: Parameter n von 0xD3 ueber das undokumentierte Register 0xD9.
static uint64_t bb_read_d3_via_d9(int pin)
{
    uint64_t value = 0;
    for (uint8_t index = 1; index <= 3; ++index) {
        digitalWrite(PIN_TFT_CS, LOW);
        bb_command(0xD9);
        bb_clock_out(0x10 + index);
        digitalWrite(PIN_TFT_CS, HIGH);
        delayMicroseconds(5);
        value = (value << 8) | (bb_read(0xD3, pin, 8) & 0xFF);
    }
    return value << (PANEL_READ_BITS - 24);
}

static void detect_panel()
{
    pinMode(PIN_TFT_CS, OUTPUT);
    digitalWrite(PIN_TFT_CS, HIGH);
    pinMode(PIN_TFT_DC, OUTPUT);
    pinMode(PIN_TFT_SCLK, OUTPUT);
    digitalWrite(PIN_TFT_SCLK, LOW);
    pinMode(PIN_TFT_MOSI, OUTPUT);
    pinMode(PIN_TFT_MISO, INPUT_PULLUP);
    delay(5);

    const int pins[] = { PIN_TFT_MISO, PIN_TFT_MOSI };
    uint64_t raw04 = 0, rawD3 = 0;
    for (int pin : pins) {
        raw04 = bb_read(0x04, pin, PANEL_READ_BITS);
        rawD3 = bb_read(0xD3, pin, PANEL_READ_BITS);
        panel_kind = panel_classify(raw04, rawD3);
        if (panel_kind == PANEL_UNKNOWN) {
            rawD3 = bb_read_d3_via_d9(pin);
            panel_kind = panel_classify(raw04, rawD3);
        }
        snprintf(panel_raw, sizeof(panel_raw), "io%d 04:%010llx d3:%010llx",
                 pin, (unsigned long long)raw04, (unsigned long long)rawD3);
        Serial.printf("[Panel] %s -> %s\n", panel_raw, panel_kind_id(panel_kind));
        if (panel_kind != PANEL_UNKNOWN) break;
    }
    if (panel_kind == PANEL_UNKNOWN) {
        // Keine ID bekommen. Treibt das Panel MISO ueberhaupt? Liest die
        // Leitung mit Pull-down nur Nullen, nachdem sie mit Pull-up nur
        // Einsen las, haengt sie offen — so beim ST7789-CYD, dessen Panel
        // keine Rueckleitung hat (Hardware-Test 30.09.2026).
        const uint64_t pulled_up = bb_read(0x04, PIN_TFT_MISO, PANEL_READ_BITS);
        pinMode(PIN_TFT_MISO, INPUT_PULLDOWN);
        const uint64_t pulled_down = bb_read(0x04, PIN_TFT_MISO, PANEL_READ_BITS);
        pinMode(PIN_TFT_MISO, INPUT_PULLUP);
        if (panel_line_floats(pulled_up, pulled_down)) panel_kind = PANEL_NO_REPLY;
        Serial.printf("[Panel] MISO %s\n", panel_kind == PANEL_NO_REPLY ? "offen" : "getrieben");
    }
    if (panel_kind != PANEL_UNKNOWN && panel_kind != PANEL_NO_REPLY
        && strcmp(panel_kind_id(panel_kind), DISPLAY_ID) != 0) {
        Serial.printf("[Panel] Achtung: Panel ist %s, Firmware-Variante ist %s\n",
                      panel_kind_id(panel_kind), DISPLAY_ID);
    }
}

void board_display_init()
{
    detect_panel();

    // Wichtig: tft.init() laeuft vor dem LEDC-Attach des Backlights, damit die
    // SPI-Peripherie samt Panel-Reset sauber hochkommt.
    tft.init();

    // Deterministischer Reset des INVON/INVOFF-Registers: fruehere Farbtests
    // koennten die Inversion sonst persistent im Panel haengen lassen.
    tft.invertDisplay(false);

    touch_input_begin();
}

const char* board_display_id()
{
    return DISPLAY_ID;
}

const char* board_panel_id()
{
    return panel_kind_id(panel_kind);
}

const char* board_panel_raw()
{
    return panel_raw;
}

void board_set_rotation(uint8_t orientation)
{
    switch (orientation) {
        case ORIENTATION_LANDSCAPE_LEFT:
            tft.setRotation(3);
            SCREEN_WIDTH  = DISPLAY_LONG_SIDE;
            SCREEN_HEIGHT = DISPLAY_SHORT_SIDE;
            break;
        case ORIENTATION_LANDSCAPE_RIGHT:
            tft.setRotation(1);
            SCREEN_WIDTH  = DISPLAY_LONG_SIDE;
            SCREEN_HEIGHT = DISPLAY_SHORT_SIDE;
            break;
        case ORIENTATION_PORTRAIT:
        default:
            tft.setRotation(0);
            SCREEN_WIDTH  = DISPLAY_SHORT_SIDE;
            SCREEN_HEIGHT = DISPLAY_LONG_SIDE;
            break;
    }
    touch_orientation = orientation;
}

lv_display_rotation_t board_lvgl_rotation()
{
    return LV_DISPLAY_ROTATION_0;   // TFT_eSPI dreht im Panel
}

void board_fill_black()
{
    tft.fillScreen(TFT_BLACK);
}

void board_flush(const lv_area_t *area, uint8_t *px_map)
{
    uint32_t w = (area->x2 - area->x1 + 1);
    uint32_t h = (area->y2 - area->y1 + 1);

    tft.startWrite();
    tft.setAddrWindow(area->x1, area->y1, w, h);
    // LVGL legt RGB565 in Host-Byte-Reihenfolge ab, TFT_eSPI schiebt MSB
    // zuerst raus. Die RGB/BGR-Reihenfolge des Panels kommt per Build-Flag
    // (TFT_RGB_ORDER) aus platformio.ini.
    tft.pushColors((uint16_t *)px_map, w * h, true);
    tft.endWrite();
}

bool board_touch_read(uint16_t *x, uint16_t *y)
{
    return touch_input_read(x, y, touch_orientation);
}

void board_lvgl_buffers(void **buf1, void **buf2, uint32_t *size_bytes)
{
    *buf1 = lv_buf1;
    *buf2 = lv_buf2;
    *size_bytes = sizeof(lv_buf1);
}

void board_backlight_init()
{
    // Reihenfolge: ledcSetup -> ledcAttachPin -> ledcWrite. Kein pinMode oder
    // digitalWrite danach, das wuerde den Pin wieder aus dem PWM-Modus reissen.
    // Channel 7 statt 0, um TFT_eSPI-internen Channels aus dem Weg zu gehen.
    ledcSetup(BACKLIGHT_LEDC_CHANNEL, BACKLIGHT_LEDC_FREQ_HZ, BACKLIGHT_LEDC_RES_BITS);
    ledcAttachPin(PIN_TFT_BL, BACKLIGHT_LEDC_CHANNEL);
    ledcWrite(BACKLIGHT_LEDC_CHANNEL, 255);  // volle Helligkeit bis NVS geladen ist
    Serial.printf("[BL] LEDC attached: pin=%d ch=%d freq=%uHz res=%ubit duty=255\n",
                  PIN_TFT_BL, BACKLIGHT_LEDC_CHANNEL,
                  BACKLIGHT_LEDC_FREQ_HZ, BACKLIGHT_LEDC_RES_BITS);
}

void board_backlight_set_percent(uint8_t pct)
{
    if (pct < BRIGHTNESS_MIN_PERCENT) pct = BRIGHTNESS_MIN_PERCENT;
    if (pct > BRIGHTNESS_MAX_PERCENT) pct = BRIGHTNESS_MAX_PERCENT;
    ledcWrite(BACKLIGHT_LEDC_CHANNEL, (uint32_t)pct * 255u / 100u);
}

bool board_persists_config()
{
    return true;
}

uint32_t board_frame_glitches()
{
    return 0;
}

#endif // BOARD_CYD
