# Website-Überarbeitung · 28.09.2026

## Gestaltungsentscheidung

Die aktuelle Runde verwendet den Skill `anti-ui-slop` und dessen Playbook `reference/distill.md`: eine kompakte Projektseite statt einer langen Folge von Werbeabschnitten. Die vorherige Prüfung mit `web-design-guidelines` bleibt Grundlage für Tastaturbedienung, Bildgrößen und Statusmeldungen.

- Kleinerer Einstieg mit Projektbezug, normaler Systemschrift und zurückhaltenden Schaltflächen.
- Gerätevorschau mit Modell- und Farbauswahl direkt darunter; S3/Graphit als Standard. Variantenlinks bleiben nutzbar.
- Der wichtigste Unterschied folgt unmittelbar: echte Firmware-Ausgabe mit drei Claude-Kontingenten, Reset und Tempoanzeige; darunter die kompakteren CYD-Layouts.
- Zwei Schreibtischbilder nebeneinander, schlichte Bildunterschriften.
- Doppelte Hardwarebilder, dekorative Abschnittsnummern, wiederholte Slogans und abschließender Download-Werbeblock entfernt.
- Hardware, Druckdateien, Downloads, Einrichtung und FAQ bleiben erreichbar.

## Funktionsumfang

Modell/Farbe verwenden native Buttons mit zugänglichen Namen und Auswahlzuständen. Varianten stehen in der URL. Bei Bildwechseln bleiben Lade- und Fehlerzustände sowie Schutz gegen verspätete Antworten erhalten. Sichtbare Tastaturfokusse und reduzierte Bewegung werden berücksichtigt. Bilder haben feste Abmessungen und Alt-Attribute.

Die S3-Bilder und die genaue Herkunft der Bildschirmdarstellung sind in [website-claude-visuals.md](website-claude-visuals.md) beschrieben. Die Veröffentlichung erfolgt über den bestehenden GitHub-Pages-Workflow beim Push auf `main`.

## Abschlussprüfung

Alle sechs Modell-/Farbkombinationen laden fehlerfrei. Deutsch/Englisch einschließlich Seitentitel und neuer Texte geprüft; Modellwahl per Enter mit sichtbarem Fokus bestätigt. Kein horizontaler Überlauf bei 320, 390 und 900 CSS-Pixeln. Die vollständige Desktop-Seite wurde visuell geprüft. Bilddateien, Sprungziele, Übersetzungsschlüssel, JavaScript-Syntax, Versionsabgleich und `git diff --check` sind geprüft. Der Firmware-Bildrenderer wurde erfolgreich ausgeführt; die Firmware selbst wurde nicht verändert.
