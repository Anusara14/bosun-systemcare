# Bosun SystemCare: logo usage

## Files
The circle B mark (`svg/bosun-tray.svg`) is the app icon everywhere: exe, taskbar, tray, lockups.
The glass tile (`svg/bosun-icon.svg`, `svg/bosun-icon-app.svg`) is kept as an alternate.

| Use | File |
|---|---|
| App / taskbar / installer icon | `ico/bosun.ico` (16–256 px) |
| Tray icon (flat, made for tiny sizes) | `ico/bosun-tray.ico` (16–48 px) |
| Master artwork | `svg/bosun-icon.svg`, `svg/bosun-tray.svg` |
| Name next to the logo | `svg/bosun-lockup-horizontal-{dark,light}.svg`, `svg/bosun-lockup-stacked-{dark,light}.svg` |
| PNGs | `png/` (icon 16–1024 px, tray 16–64 px, lockups) |

## Colours
| Name | Hex | Use |
|---|---|---|
| Ring Blue | `#1E9CFF` | Ring, glow |
| Glass Deep | `#0B0D10` → `#0C4F8C` → `#2A9BF0` | Tile gradient (top → bottom) |
| Core Black | `#05070A` | Inside the ring |
| Glyph White | `#FFFFFF` → `#D6ECFF` | The B |
| UI accent | `#60CDFF` | Buttons and rings inside the app (Windows 11 dark accent) |
| UI charcoal | `#202020` / `#2B2B2B` | App background / cards |

## Rules
- Clear space: keep at least 1/4 of the icon's width empty on every side.
- Minimum size: use the tile icon at 32 px or larger. Below 32 px, use the tray version.
- Lockups: the dark version on dark backgrounds and the light version on light ones. Don't recolour the tile.
- Don't stretch it, rotate it, change the B, or add effects.

## Type
The wordmark is Segoe UI Bold ("Bosun") plus Segoe UI Regular ("SystemCare"), converted to outlines.
Segoe UI is a Microsoft font. Before a commercial release, check that its licence allows use in a logo, or swap in an openly licensed font such as Inter or Open Sans and re-run `../build_lockup.py`.
