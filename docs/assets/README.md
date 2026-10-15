# EdgeHome visual identity

## Design direction — Quiet Threshold

The identity uses architectural geometry to describe a controlled boundary. A single house silhouette contains a shared horizontal bar and an open threshold. The house carries the local-home context; the bar and small square suggest a command passing through a deliberate point of control. The symbol remains legible without letters, chip pins, or a separate shield.

Deep ink, warm off-white, and a restrained green establish the visual hierarchy. Green marks the threshold and connective elements rather than implying that every model proposal is approved. The carefully aligned silhouette and generous surrounding space keep the mark readable at small sizes.

Typography balances a substantial sans-serif title with quiet monospaced annotations. The banners use Instrument Sans and DM Mono, converted into vector paths for consistent rendering without external font requests. Their spacing and baseline alignment are intentionally tuned to make the brand feel precise and calm.

Light and dark compositions share the same geometry. Refinement comes from consistent spacing, stroke weight, and contrast rather than extra decoration. The standalone icon has a solid ink tile so its silhouette remains stable across GitHub themes and avatar backgrounds.

## Assets

| File | Use |
| --- | --- |
| `edgehome-logo.svg` | Primary square icon; scalable, with a solid background |
| `edgehome-logo.png` | 1024px avatar/export version of the primary icon |
| `edgehome-mark.svg` | Transparent standalone symbol for light backgrounds |
| `edgehome-banner.svg` | Light README banner, 1280 × 480 |
| `edgehome-banner-dark.svg` | Dark README banner, 1280 × 480 |
| `edgehome-harness-overview.jpg` | Earlier architecture illustration, retained for existing references |

The README selects its banner with a `<picture>` element and a dark-mode media query. Every SVG has a title and description, uses local vector geometry, and contains no scripts, external resources, or embedded fonts. Banner lettering is outlined; accessible equivalents are supplied through image alt text and SVG descriptions.

Use the square icon at 32px or larger. Keep clear space around the icon; avoid adding borders, shadows, or another enclosing shield. The transparent mark is intended for light backgrounds; use the primary square icon on dark backgrounds.

## Font attribution

Banner lettering is derived from **Instrument Sans** and **DM Mono**, distributed under the SIL Open Font License 1.1. The original notices and license terms are retained in [InstrumentSans-OFL.txt](InstrumentSans-OFL.txt) and [DMMono-OFL.txt](DMMono-OFL.txt). The icon contains no font glyphs.
