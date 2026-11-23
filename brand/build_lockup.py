"""Builds Bosun SystemCare lockups with the wordmark as outlined paths (no live text)."""
import re, sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

def text_path(font_file, text, size, x, baseline):
    f = TTFont(font_file); gs = f.getGlyphSet(); cmap = f.getBestCmap()
    s = size / f["head"].unitsPerEm; d = []
    for ch in text:
        g = cmap[ord(ch)]; pen = SVGPathPen(gs)
        gs[g].draw(TransformPen(pen, (s, 0, 0, -s, x, baseline)))
        d.append(pen.getCommands()); x += gs[g].width * s
    return " ".join(d), x

icon = open("bosun-tray.svg", encoding="utf-8").read()
icon_inner = re.sub(r"^<svg[^>]*>|</svg>\s*$", "", icon.strip())
ICON = f'<svg x="0" y="0" width="260" height="260" viewBox="0 0 260 260" overflow="visible">{icon_inner}</svg>'
F = "C:/Windows/Fonts/"

def lockup(name, txt1, txt2, stacked):
    if stacked:
        size = 64
        w1 = text_path(F + "segoeuib.ttf", "Bosun", size, 0, 0)[1]
        w2 = text_path(F + "segoeui.ttf", " SystemCare", size, w1, 0)[1]
        x0 = 130 - w2 / 2; base = 380
        p1, e1 = text_path(F + "segoeuib.ttf", "Bosun", size, x0, base)
        p2, e2 = text_path(F + "segoeui.ttf", " SystemCare", size, e1, base)
        vb = f"{min(-40, x0 - 20):.0f} -40 {max(340, w2 + 40):.0f} 460"
    else:
        size = 100; base = 165
        p1, e1 = text_path(F + "segoeuib.ttf", "Bosun", size, 300, base)
        p2, e2 = text_path(F + "segoeui.ttf", " SystemCare", size, e1, base)
        vb = f"-40 -40 {e2 + 80:.0f} 340"
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}">{ICON}'
           f'<path fill="{txt1}" d="{p1}"/><path fill="{txt2}" d="{p2}"/></svg>')
    open(name, "w", encoding="utf-8").write(svg)

lockup("bosun-lockup-horizontal-dark.svg", "#FFFFFF", "#C5C5C5", False)
lockup("bosun-lockup-horizontal-light.svg", "#111111", "#3A3A3A", False)
lockup("bosun-lockup-stacked-dark.svg", "#FFFFFF", "#C5C5C5", True)
lockup("bosun-lockup-stacked-light.svg", "#111111", "#3A3A3A", True)
print("ok")
