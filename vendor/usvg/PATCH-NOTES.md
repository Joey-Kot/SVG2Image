# Local text fallback fixes

SVG2Image carries three focused fixes on top of `usvg 0.48.1`:

- a fallback font replaces only glyphs missing from the primary font, rather than replacing an entire mixed-script run;
- missing glyphs first follow the SVG's declared `font-family` list in order, using the requested style, weight, and stretch, before falling back to an unrelated system font;
- fallback candidates are ranked by style, monospace behavior, weight, and stretch instead of accepting an arbitrary face whenever any one property matches.

The upstream behavior can turn a run such as `y, 这是Test` entirely into the first CJK font found on the system, changing both the Chinese and Latin glyphs and sometimes selecting the wrong weight. With `font-family="Rubik, 'Noto Sans SC'"`, Latin glyphs now remain in Rubik while missing Chinese glyphs use the matching Noto Sans SC face.
