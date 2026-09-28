# Website-Bilder und Firmware-Ansichten

Die Produkt- und Schreibtischbilder wurden am 28.09.2026 mit der integrierten Bildgenerierung (image_gen) erstellt. Die bestehenden Fotos bleiben unverändert als Referenz im Projekt; die Website verwendet die neuen JPEGs in `installer/assets/renders/`.

Die spätere S3-Überarbeitung mit vollständiger Claude-Ansicht ist in [website-claude-visuals.md](website-claude-visuals.md) dokumentiert. Die folgenden Prompts beschreiben die erste Bildserie.

## Formvorlagen

- CYD: eigene Fotos `display-chatgpt-front.jpg` und `display-chatgpt-side.jpg`.
- S3: eigenes Foto `display-s3-chatgpt.jpg`; Form und Ständer nach [Case Stand V3 von cardboxed](https://makerworld.com/de/models/2581572-guition-esp32s3-4848s040-case-stand-v3#profileId-3013482). Die fremden Referenzfotos werden nicht auf der Website veröffentlicht.
- Gestalterische Referenz: [BLINK](https://blink-buddy.com/) – große Produktansichten und kurze inhaltliche Abschnitte; eigenständige Gestaltung mit Weiß, Graphit, Kobaltblau, geradliniger Typografie und offenen Bildabschnitten. Die warmen Töne bleiben auf die Produkt- und Schreibtischbilder beschränkt.

Die Bilder sind als KI-Visualisierungen gekennzeichnet. Farben sind Druckideen, keine angebotenen Kaufvarianten. Beim S3 ändert sich nur der gedruckte hintere Ständer, die originale silberne Einfassung und die schwarze Glasfront bleiben erhalten. Die generierten Ansichten sind keine maßhaltigen CAD-Renderings.

## Tatsächliche Firmware-Ausgabe

`installer/assets/screens/` enthält direkt mit LVGL gerenderte PNGs. Verwendet werden die unveränderten `src/ui_dashboard.cpp`, `src/ui_common.cpp`, Provider, Lokalisierung und Schriften. Nur Hardware-I/O und Uhrzeit werden im Host-Werkzeug ersetzt. Demo: 67 % Woche, 32 % Sitzung, feste Uhrzeit; keine persönlichen Nutzungsdaten.

Reproduzieren auf macOS (CMake, C/C++-Compiler, sips sowie vorhandene, projektgebundene LVGL-/ArduinoJson-Abhängigkeiten aus dem CYD-Build):

```sh
bash scripts/render_site_screens.sh
```

Die PNGs sind echte Firmware-Layouts. In den fotografischen KI-Bildern dienen diese PNGs als Bildschirmreferenz; sie sind dort perspektivisch generiert und nicht pixelidentisch einkopiert.

## Finale Prompts

### cyd-blue.jpg

Use case: compositing / product-mockup. Edit input 1, the blue ESP32 CYD studio product photo. Preserve its exact hardware geometry, four black hex screws, camera angle, USB cable, warm off-white background, soft contact shadows and framing. Replace the screen content with input 2, the actual firmware screenshot. Perspective-map the COMPLETE screenshot exactly into the LCD area, without redesigning it: CLAUDE, 14:32, two rings 'Woche' 67% and 'Sitzung' 32%, reset text. Screen is matte and readable with zero glare. Refine the enclosure material to high-quality matte cobalt-blue PLA with delicate uniform layer lines rather than coarse diagonal scratched-looking marks. Photorealistic commercial product shot, landscape 3:2. No text outside screen, no props, no watermark.

### s3-graphite.jpg

Use case: product-mockup. Generate photorealistic commercial studio photograph of the square 4-inch Guition ESP32-S3-4848S040 display mounted in the exact wedge-shaped 3D printed case stand from inputs 1 and 2. Input 1 is front and right side geometry reference ONLY; ignore its Home Assistant screen, replace screen with input 3, actual AI Monitor firmware screenshot. Input 2 shows exact rear triangular wedge shape and bottom cable notch, keep that design. Square flat front with glossy black bezel and thin silver edge, NO front screws, leaned back approximately 15 degrees, deep flat triangular side cheeks forming rear stand. Stand matte graphite PLA with fine print layer lines. Camera front three-quarter with RIGHT side of wedge clearly visible. Small white USB cable attached right side. Screen maps input 3 exactly: CHATGPT header,14:32, green check, warm-white background, cyan 67% ring, Woche, 3 Tg. 8 Std., Donnerstag, 22:32 Uhr. No glare on screen. Device centered and occupies 70% of landscape 3:2 image, all edges visible. Seamless warm off-white #f3f0e9 studio backdrop and matte surface, soft contact shadow, large diffused upper-left light. No other objects, no external text, no watermark.

### cyd-graphite.jpg

Use case: precise-object-edit. Edit the supplied studio product photograph. Change ONLY the blue 3D-printed enclosure surrounding the screen to matte deep graphite gray.  Keep the same fine PLA layer texture, exact shape, all hardware details, screen content and text EXACTLY unchanged. Preserve all camera angle, perspective, composition, background, lighting, contact shadows, cable and landscape 3:2 image dimensions. Do not redesign or change the screen. Photorealistic studio product photography. No additional objects, typography or watermark.

### cyd-terracotta.jpg

Use case: precise-object-edit. Edit the supplied studio product photograph. Change ONLY the blue 3D-printed enclosure surrounding the screen to matte muted terracotta clay orange.  Keep the same fine PLA layer texture, exact shape, all hardware details, screen content and text EXACTLY unchanged. Preserve all camera angle, perspective, composition, background, lighting, contact shadows, cable and landscape 3:2 image dimensions. Do not redesign or change the screen. Photorealistic studio product photography. No additional objects, typography or watermark.

### s3-blue.jpg

Use case: precise-object-edit. Edit the supplied studio product photograph. Change ONLY the 3D-printed wedge stand BEHIND the silver housing, visible along the right/rear side to matte cobalt blue. Keep the factory SILVER housing and BLACK glass front bezel unchanged; only the printed rear stand changes color. Keep the same fine PLA layer texture, exact shape, all hardware details, screen content and text EXACTLY unchanged. Preserve all camera angle, perspective, composition, background, lighting, contact shadows, cable and landscape 3:2 image dimensions. Do not redesign or change the screen. Photorealistic studio product photography. No additional objects, typography or watermark.

### s3-terracotta.jpg

Use case: precise-object-edit. Edit the supplied studio product photograph. Change ONLY the 3D-printed wedge stand BEHIND the silver housing, visible along the right/rear side to matte muted terracotta clay orange. Keep the factory SILVER housing and BLACK glass front bezel unchanged; only the printed rear stand changes color. Keep the same fine PLA layer texture, exact shape, all hardware details, screen content and text EXACTLY unchanged. Preserve all camera angle, perspective, composition, background, lighting, contact shadows, cable and landscape 3:2 image dimensions. Do not redesign or change the screen. Photorealistic studio product photography. No additional objects, typography or watermark.

### desk-cyd.jpg

Use case: photorealistic-natural / product-mockup. Generate a sophisticated but lived-in developer desk editorial photo in landscape 3:2. The small device from the reference image is the hero subject: exact ESP32 CYD shape with 4 black screws, matte cobalt blue 3D-printed case and SAME Claude screen. Place this palm-sized 2.8-inch screen device directly UNDER a large 27-inch computer monitor, on its flat silver monitor base, slightly to one side of the vertical monitor support. Realistic scale: device case roughly 10cm wide, monitor roughly 60cm wide. Screen legibly shows CLAUDE two rings Woche 67%, Sitzung 32%. A discreet black USB cable leaves the device and runs toward the back of the desk. Calm daylight from left, light oak desk, warm offwhite plaster wall, slim silver keyboard partly in foreground and a small notebook at far side. Main monitor shows out-of-focus dark code editor, no readable external brand logos. Medium close-up at desk level, CYD sharply in focus lower center, monitor lower edge and support clearly visible above it; photographic depth of field, natural soft shadows, no display reflections obscuring UI. Quiet, premium, believable workspace, no neon, no RGB gaming lights, no plants covering the product. Do not enlarge device into a tablet. No overlaid text, no watermark.

### desk-s3.jpg

Use case: photorealistic-natural / product-mockup. Create a premium editorial mood photograph in landscape 3:2 of the exact square 4-inch Guition ESP32-S3 desk display in the reference image. Preserve black glass bezel, thin silver side housing, dark graphite printed wedge-shaped Case Stand V3 with triangular side, and white USB cable on right. Preserve reference screen contents: CHATGPT, warm white, cyan Woche 67% ring and German reset text. Place the small roughly 12cm wide display beside the right edge of a slim silver keyboard on a warm walnut desk. Background includes corner of a monitor with blurred code editor, soft evening window light and a small warm desk lamp beyond focus. Natural eye-level three-quarter view, with right side of wedge stand visible, device prominent and sharply focused but correctly scaled relative to keyboard. Tactile desk surface, gentle real shadows, subtle warm tones, calm developer workspace; no RGB/neon, no invented extra buttons or screws, no dark unreadable display, no brand logos, no overlaid text, no watermark.

