# WMP as the answer key

GitHub's `windows-2025` runners ship Windows Media Player Legacy. The `WMP` workflow (manual,
Actions tab) runs it so a rule in either engine can be measured instead of guessed.

| task | what it does | output |
| --- | --- | --- |
| `probes` | runs each test skin in `probes/` | per probe: `elements.tsv` (UI Automation boxes), `black.png`/`white.png`, click and GIF results in the log |
| `layout` | opens every skin in the archive.org collection | `layout.tsv`: every skin element's box as UI Automation reports it |
| `capture` | the same skins | screenshots over black, white and black again, and `windows.tsv` |

Compare a layout run with the engine:

    webskin --test-skins <skins> out
    python3 tools/wmp/layoutcmp.py layout.tsv out

A probe is a folder holding a `.wms`, its images and `probe.txt`:

- `view=` the view id to find
- `clicks=target:marker,...` clicks the target's centre (or `target@x;y`) and reads the marker
  element's colour before and after; marker `*` counts the window pixels that changed
- `songs=N` plays N generated tones first (installs the VB-Cable audio driver, since the runner
  has no sound device and WMP won't play without one)
- `sample=gifs` with `delays=` times GIFs drawn as 60x60 squares 70 px apart

How it works: a registry policy skips WMP's first-run wizard; `wmplayer.exe /layout:skin.wmz`
offers the skin and UI Automation presses "View Now"; skin windows are UI Automation elements of
class `WMP Skin Host` named by view id, and their descendants are the skin's elements by id; clicks
are `SetCursorPos` and `mouse_event`; a full-screen window behind WMP turns black or white so clear
pixels can be told from drawn ones.

## What the probes found

| probe | WMP |
| --- | --- |
| `faded-parts` | a subview at alphaBlend 0 or 1 still blocks clicks to the button under it |
| `visualizers` | an EFFECTS blocks clicks, windowed or not, behind a hole or not, faded or not |
| `faded-visualizers` | the same when the fade comes from `alphaBlendTo`, and over button groups |
| `passthrough-visualizers` | an EFFECTS in a passThrough subview lets the click through |
| `passthrough-children` | a button in a passThrough subview takes no click; a nested subview with a fill still blocks |
| `passthrough-inherited` | a passThrough subview with a fill passes clicks; passThrough doesn't reach into nested subviews; a child's own `passThrough="false"` doesn't win |
| `pictureless-buttons` | a button with no picture, or a missing one, is 192x192 and takes clicks across it |
| `listbox` | a Windows list: rows 1.3 times the font's pixel size, 1 px #7A7A7A border, selection #0078D4 with white text; a click selects; ITEM children come first |
| `gif-timing` | a frame shows for its delay rounded up to the 1/64 s timer tick, at least 3 ticks: 0-40 ms all show 47 ms |
| `playlists` | the playlist control's header, rows and colours with songs in it |

Also measured with the `capture` and `layout` tasks: WMP shapes the window from the backgrounds,
so anything a control draws over a background's hole is cut (xXx Night Vision's lime corners);
TEXT defaults to 10 points, rounded to whole pixels.
