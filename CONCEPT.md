# Box Packing Konzept

## Parameter

Quader sollen möglichst effizient in einem Quader verpackt werden.

## Relevante Werte

Maximalgewicht Verpackung.
Gewicht Einzelobjekte.
Abmessung (3d) Verpackung.
Abmessung (3d) Einzelobjekte.
Mehrere Verpackungstypen mit individuellen Dimensionen und Gewichtslimits.

## Ziel

Algorithmische Lösung.
Bei zu geringem Volumen oder Grundfläche der Verpackung sollen die Objekte entsprechend in mehreren Verpackungen der angegebenen Größe möglichst effizient verpackt werden.

Entsprechend muss dann auch der Algorithmus mehrfach ausgeführt werden, bis alle Objekte verpackt sind.
Zudem kann der Algorithmus unterschiedliche Verpackungstypen kombinieren, um den Bedarf bestmöglich abzudecken.

Schwere Objekte müssen immer unter leichteren Objekten sein, das Gewicht muss ebenfalls gleichmäßig auf der Grundfläche verteilt werden

Große objekte nach möglichkeit nach unten. Die grundfläche soll möglichst gleichmäßig mit gewicht belastet sein und möglichst gleichmäßig mit objekten gefüllt sein. Es dürfen keine Objekte überhängen, so dass sie herunterfallen könnten.

Am ende sollen die Objekte möglichst raumfüllend und kompackt gepackt sein.

Objekte können nicht gedreht werden.

## Tech Basis

Rust Konsolen App mit andauernder Laufzeit und ansprechbaren Schnittstellen

3D-Geometrische Heuristik in Kombination mit Gewichtsverteilung und Schichtung
