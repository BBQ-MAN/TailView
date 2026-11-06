# Mechanics sheet (from BandiView.exe build 34522)

Values read from the original program's code with Ghidra. Addresses are in BandiView.exe.
Anything marked **estimated** was looked for in the code and not found.

## Zoom (FUN_1400f9950, FUN_14013bc10, FUN_14013bcc0)

- Zoom is stored as a fixed-point integer: `10000` = 100 %.
- Zoom in / out commands pass a step of 10 percentage points (`1000` units); "by 1%" commands pass `100`.
  FUN_14013bc10 converts: `units = step / 100.0 * 10000.0 ± 0.5` (constants 0x140671da0 = 100.0,
  0x140671eb0 = 10000.0, 0x1406719a8 = 0.5).
- With a 1 % step the zoom simply adds the step (minimum 1 %).
- Zoom in with a 10 % step (FUN_14013bcc0):
  `inc = 1000 if z < 30000, 2000 if z < 50000, else 5000`; `z' = round((z + inc) / step) * step`.
  So 10 % steps below 300 %, 20 % steps up to 500 %, 50 % steps above.
- Zoom out: `dec = 1000 if z <= 30000, 2000 if z <= 50000, else 5000`; `z' = round((z - dec) / step) * step`.
- Clamp: minimum `500` (5 %), maximum `2000000` (20000 %).
- Zoom-in command when the zoom is below 10 % jumps straight to 10 % (FUN_1400f9950 case 2).
- "Set 100 %" sets `10000` (case 6). "Reset zoom" drops back to the view mode's fit.
- When zooming from a fit mode the current fit ratio is taken as the starting zoom (FUN_14013bcc0).
- Zoom is anchored at the mouse cursor when it is inside the window, otherwise at the centre (FUN_14013c100).

## Webtoon mode (FUN_14007a7e0 settings loader)

- Settings keys: `viewMode.applyWebtoonWidth`, `viewMode.webtoonWidth`, `webtoonSpace`, `setwebptoonspace`.
- Width presets `webtoon.preset.w-0..2` default to **690, 760, 800** px (table at 0x1405ff4c0).
- Space key = long scroll down; PageUp/PageDown scroll (CMD_KEY_SPACE / CMD_KEY_PAGEDN strings).
- Defaults (FUN_1400796e0, view-mode reset): apply-width **off**, width 690 px.
- Spacing (FUN_14007a090): off, 10 px when on (`webtoonSpace` = 10).
- With apply-width off each page keeps its own width, shrunk to the window — **estimated**.
- Long scroll = 90 % of the window height, arrow scroll = 15 % — **estimated**.

## Slideshow (FUN_14010d330, FUN_14010df90)

- 9 interval slots `slideShowTime[0..8]` in milliseconds, bound to Ctrl+1..Ctrl+9; Ctrl+0 stops.
- Label shows whole seconds when `ms % 1000 == 0`, else one decimal.
- Timer ticks every 50 ms (SetTimer 0x74, 0x32).
- Options: loop, random, show progress bar. Page-turn effects: none / flip / fade / slide.
- Defaults (FUN_14007a090): slots 1000, 2000 … 9000 ms; loop on, random off, progress bar off.

## Ads (removed)

- FUN_140067690 logs `@@rLaunchPopupAdWindow()` and opens a pop-up ad once per day
  (compares the day number stored at settings+0x2c).
- data/local.html is the "Buy BandiView Professional" ad panel; data/coupang.exe is a shopping ad launcher.
- The rebuild has no ad code, no web panels and no network calls.

## Sorting (FUN_14007a7e0)

- `sort.method`, `sort.ascending`, `folder_sort.method`. Methods: name, date modified, size, type,
  date taken, as Explorer does, created.

## Image effects (FUN_14007a7e0)

- `viewMode.gamma1000`, `brightness100`, `contrast100`, `saturation255`; toggles negative, gamma,
  flip, mirror, gray, sepia, RGB channel, rotate.
- Gamma compensation: `viewMode.gamma1000` = 450, i.e. exponent 0.45 (FUN_1400796e0).

## Window and look (FUN_14007a090, settings reset)

- Window 1000 × 800 px at 96 dpi, centred on the work area.
- Background colour `clrBkgnd` = 0x202020; transparent-image tiles 0xFFFFFF / 0xC8C8C8.
- Title bar format `${folder} ${sep} ${file} [${page}/${total}]`.
- Mouse wheel without Ctrl moves to the next / previous image — **estimated**.
