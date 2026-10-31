<p align="center">
  <img src="docs/images/banner.png" width="880" alt="WebSkin Player: classic Windows Media Player skins and visualizations for the music playing in your browser">
</p>


Original Windows Media Player skins (`.wmz`) and WMP-style visualizations for whatever your browser is playing:
SoundCloud or Spotify in Chrome, Edge, or any app that shows up in the Windows media controls.

Microsoft is ending skin support in Windows Media Player Legacy on November 10, 2026
([notice](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wmp/effects-windowed)).
WebSkin Player re-implements the skin engine, so those skins keep working, with your web music instead of local files.

<p align="center">
  <a href="docs/images/desktop.png"><img src="docs/images/desktop.png" width="880" alt="Seventeen classic Windows Media Player skins spread over a green-hills desktop, all playing the same song"></a><br>
  <sub>Skins by Microsoft and their original designers, each captured running in WebSkin Player and arranged on one desktop.</sub>
</p>

## Take the tour

**Every skin runs its own scripts**, so its buttons, sliders, drawers, animations and visualizer behave as they did in
WMP, following the song in your browser.

<table>
  <tr>
    <td align="center" width="50%"><img src="docs/images/skin-heart.gif" width="380" alt="The Heart skin playing a song with the Ambience visualization"><br><b>Heart</b> with Ambience</td>
    <td align="center" width="50%"><img src="docs/images/skin-fishbowl.gif" width="380" alt="The Fishbowl skin with the Plenoptic visualization in its bowl"><br><b>Fishbowl</b> with Plenoptic</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/skin-windowsxp.gif" width="380" alt="The Windows XP skin with the Alchemy visualization"><br><b>Windows XP</b> with Alchemy</td>
    <td align="center"><img src="docs/images/skin-plusbionicdot.gif" width="380" alt="The Plus! Bionic Dot skin with the Eclectic Colors visualization"><br><b>Plus! Bionic Dot</b> with Eclectic Colors</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/skin-muktuk.gif" width="380" alt="The Muktuk skin with the StarTime visualization"><br><b>Muktuk</b> with StarTime</td>
    <td align="center"><img src="docs/images/skin-splat.gif" width="380" alt="The Splat skin with the Plenoptic visualization"><br><b>Splat</b> with Plenoptic</td>
  </tr>
</table>

<table>
  <tr>
    <td width="50%"><img src="docs/images/visualizations.gif" width="420" alt="The Compact skin running through ten visualizations: Bubbles, Plenoptic, Distortion, ColorCubes, Album Art 3D, 3D Alchemy, up cuber, BlazingColors, Alchemy and Battery"></td>
    <td>
      <b>Visualizations from every WMP era.</b> Built in: WMP's own Alchemy (from Windows Media Player Legacy) and
      faithful Bars and Waves and Battery. Add the old 32-bit plug-ins people collected, like WMP 8's Ambience and
      Plenoptic, the Psychedelia pack or StarTime, straight from their <code>.dll</code>, <code>.zip</code> or
      <code>.msi</code>; nothing gets installed into Windows. Each skin remembers the one you last showed in it, and
      any visualization can be recoloured: Fire, Ice, Neon, a green or amber monitor, or a colour of your own.
      <a href="#visualizations">More</a>
    </td>
  </tr>
</table>

<table>
  <tr>
    <td align="center" width="50%"><img src="docs/images/full-mode.png" width="400" alt="Full mode: the Alchemy visualization filling the window, with the song title above and the transport bar below"><br><b>Full mode</b> (Ctrl+1): a big visualizer, double-click for full screen</td>
    <td align="center" width="50%"><img src="docs/images/skin-chooser.png" width="400" alt="The Skin Chooser listing installed skins by publisher, previewing Fishbowl"><br><b>Skin Chooser</b>: live previews, skins grouped by publisher, the archive.org collection a click away</td>
  </tr>
</table>

<p align="center">
  <img src="docs/images/widget.gif" width="460" height="48" alt="The taskbar widget: album art, title and artist, a moving mini spectrum, and previous, play/pause and next buttons"><br>
  <b>Taskbar widget</b> (Ctrl+3): the song, a live mini spectrum and the transport buttons, docked in the Windows taskbar.
</p>

**And a visualizer screen saver**: when the PC sits idle, every monitor shows a random visualization with the current
song fading in. [More](#screen-saver)

## Install

<a href="../../releases/latest"><img src="docs/images/download.png" width="300" height="76" alt="Download WebSkin Player, free for Windows 10 and 11"></a>

1. Download **`WebSkinPlayer-<version>-Setup.exe`** from the [latest release](../../releases/latest).
2. Run it. It installs for your Windows account only, needs no admin rights and no separate .NET install, and adds a
   **WebSkin Player** Start menu shortcut.
3. Start playing something in the browser. The player picks it up automatically.

Notes:
- **Windows SmartScreen** may say it "protected your PC", because the installer isn't code-signed. Choose
  **More info → Run anyway**.
- **Opening `.wmz` files:** Windows asks which app to use the first time you open one. Pick **WebSkin Player** and
  tick **Always**.
- **Updates:** the app checks this repository's releases when it starts and shows **Update available** in full mode
  (and in a skin's right-click menu). **Install Now** downloads the new Setup.exe, checks it against the release's
  checksum, and runs it: the player closes, upgrades in place and starts again, keeping your skins and settings.
  **What's New** opens the release page. A portable copy gets the release page instead (download the new zip).
- **Uninstall** from Settings → Apps. It asks whether to delete your downloaded skins and settings too.
- **Portable:** the release also has a `portable.zip`. Unzip it anywhere and run `WebSkinPlayer.exe`. Put an empty
  file named `portable` next to the exe to keep settings and skins in a `Data` folder beside it.

Requires 64-bit Windows 10 (version 2004 or later) or Windows 11.

## Getting skins

No skins are bundled. They belong to their original authors.

- **Skin Chooser → Get more skins** lists the 175-skin
  [Windows Media Player Skins Collection](https://archive.org/details/windowsmediaplayerskinscollection) on archive.org,
  with a screenshot of each skin taken in WMP. Download one (**Download and Apply**) or all of them (about 300 MB).
  On a new install the app opens there.
- **Install skin file…** accepts `.wmz`, `.wms` and `.zip` (every `.wmz` inside a zip is installed). Opening a `.wmz`
  from Explorer installs it and switches to it.
- [WMP Goodies](https://sites.google.com/view/wmpgoodies/skins) lists another 266 skins (about 150 not in the
  archive.org collection). Download them from there and use **Install skin file…**.

The Skin Chooser groups skins by **publisher**: the company a skin was made for (Microsoft, Disney, Warner Bros.,
Alienware...), with skins by individual artists under *Independent artists*. Pick one publisher from the list under
the search box, or type a publisher or designer name into the search. A skin's details show who designed it, e.g.
*Disney · designed by The Skins Factory*. Publishers come from each skin's own copyright line, with corrections in
`src\WebSkinPlayer\Skins\SkinCatalog.json`.

## Modes (as in WMP)

| Mode | How | What |
|---|---|---|
| Full mode | Ctrl+1 | Now Playing with a large visualizer, plus the **Skin Chooser** (live previews, *Apply Skin*). |
| Skin mode | Ctrl+2 | The selected `.wmz` skin, fully interactive. Right-click a skin for the app menu. |
| Taskbar widget | Ctrl+3 | Docks in the Windows taskbar: art, title/artist, live mini-spectrum, previous/play/next. Click to open the player, scroll for volume, drag to move it, right-click for options. *Keep widget on the taskbar* shows it in every mode. |

Other shortcuts: Ctrl+P play/pause, Ctrl+S stop, Ctrl+B / Ctrl+F previous / next, F8 mute, F9 / F10 volume.
In full mode, double-click the visualizer (or press Alt+Enter) for full screen. Esc exits full screen.

The player follows whatever is playing. To pin one app instead, use **Source** in full mode. A pinned app that drops
out for a moment is waited for (15 seconds) rather than replaced by another app that's playing.

**Always on Top** keeps the player above other windows, in full mode and in every skin window. Switch it with the
pin button in full mode's transport bar, or from the right-click menu of a skin, the visualizer, or the taskbar widget.
Switch it off to let other windows cover the player again. The setting is remembered.

## The skin engine

It re-implements WMP's skin format:
- **Elements:** views, subviews, buttons and button groups (mapping images), sliders, custom sliders, text,
  effects, video, playlist, popups, equalizer and video settings.
- **Scripting:** `jscript:` expressions, `wmpprop:` / `wmpenabled:` bindings, `*_onchange` and player events, all
  running the skins' own JScript.
- **Resources:** `res://wmploc.dll/...` strings, images and scripts come from the PC's Windows Media Player.

It also reproduces the WMP quirks that skins depend on:
- Names in scripts are case-insensitive.
- A property can be read with call syntax, e.g. `mediacenter.effectType()`.
- When `scriptFile` is omitted, the `.js` named like the `.wms` loads by default.
- A VIEW's `clippingColor` and `transparencyColor` key the whole composed window.
- A subview's `clippingColor` (with no `clippingImage`) cuts its background image into the subview's shape, which
  also clips its children.
- `onmouseout` fires only when the pointer leaves an element, not when it moves onto one of the element's children
  (Revert fades its window on the view's `onmouseout`).
- Setting `theme.currentViewID` switches to another view.
- Preview views run with no `player` object, so the Skin Chooser shows them as WMP did.
- `WMPImage_AlbumArt*` shows the current track's artwork.
- Damaged zip headers are tolerated.
- Animated GIFs play with their own frame timing, disposal, and loop count. Intro animations and shutters that play
  once and end transparent (Kenwood, Blinx, STALKER) uncover what's underneath.
- Visualizer names from WMP 7/XP (Spikes, Ambience, Musical Colors, Plenoptic, Particle) map to the closest current
  effect, and the skin still reads back the name it set, so its own mode logic works.

Many skins show the visualizer only on demand, exactly as in WMP: Kenwood defaults to track info (press
**Previous Preset**, the top arrow in the right-hand cluster), Mecha and Plasma Ball have a visualizer button, and
Gold slides it out in a drawer.

**Compatibility:** all 336 skins it's tested against (the archive.org collection plus WMP Goodies' skins) load. 257
run their scripts without any error. The other 79 hit bugs in their own scripts, such as references to elements the
skin doesn't define. WMP silently ignored those errors, and so does this app; the errors are written to the log only.

**Windows Media Player Legacy** (Settings → System → Optional features) supplies WMP's built-in button images and
strings that some skins borrow, and the Alchemy visualizer. Without it, skins still load but those borrowed pieces
are blank and Alchemy isn't listed.

## Screen saver

**Screen Saver** (in the right-click menu of the visualizer, a skin, or the taskbar widget) turns on the visualizer
screen saver. Once the PC has been idle for the chosen time (1 to 30 minutes), every monitor shows a random
visualization, switching to another one every 15 seconds to 5 minutes, with the current track fading in at the bottom.
Moving the mouse or pressing a key closes it. **Lock the PC When It Closes** locks Windows as it closes, like a
screen saver with "On resume, display logon screen". **Start Now** shows it straight away.

Like Windows' own screen saver it stays away while a full-screen game, presentation or video is running, and while
the PC is locked. Music playing in a browser tab doesn't hold it back. Windows doesn't let apps draw on the lock
screen itself, which is why this is a screen saver.

## Visualizations

- **Alchemy:** the real WMP plug-in from `mpvis.dll`, hosted and fed system audio.
- **Bars and Waves:** Bars, Ocean Mist, Fire Storm, and Scope. **Battery:** 8 presets. **Album Art.**
  WMP's own Bars and Battery are locked inside `wmp.dll`, so these are faithful recreations.
- Any other WMP visualization plug-in registered on the PC appears automatically.
- **Each skin remembers its visualization.** A skin opens with the one last shown in it; a skin you haven't used yet
  starts with its own (or WMP's default, Bars). Pick one from a skin's right-click Visualizations menu and the skin
  switches to it; pick one from the list in full mode (Ctrl+1) and it's the one the skin shows when you go back. Skins that name a visualization as they switch theirs on (Kenwood sets Spikes every time it
  leaves its song display) get the remembered one instead, once there is one. Song changes and skips don't count as a
  pause and resume, even when the browser reports a moment of pause between songs or drops its media session for
  several seconds (Chrome does at some SoundCloud song changes), so skins don't drop back from their visualizer at
  every song. A pause made in the player shows at once; one made in the browser or the Windows media flyout shows
  after 1.5 seconds, as skips from there look like a pause until the next song loads. Gaps longer than a second are
  written to the log (`Playback stopped for ...`). A plug-in that duplicates a built-in visualization (WMP 8's own Bars and Waves and
  Battery) isn't listed a second time.
- **Colors.** Any visualization, the plug-ins included, can be recoloured from **Colors for ...** in a skin's
  right-click menu or full mode's visualizer menu, or the palette button next to full mode's preset list:
  - **Palettes:** the picture's brightness mapped onto a gradient: Fire, Sunset, Gold, Rose, Neon, Ice, Ocean,
    Emerald, Green Screen and Amber Screen (the old monochrome monitors), Black and White.
  - **Shift Colors:** rotates every hue by 60° to 300°, keeping the brightness.
  - **Custom Color…:** a colour of your own from Windows' colour picker.

  Each visualization keeps its own colours (Alchemy can stay green while Bars turns blue), in skins, full mode and the
  screen saver alike. **Original Colors** shows it as it was drawn.
- **Old 32-bit plug-ins** from the WMP 7 to 10 era (WMP 8's original Ambience, Particle, Plenoptic and Spikes,
  3D Alchemy, the Averett & Associates packs, the Psychedelia pack, community ones) run in a small 32-bit helper
  (`VisHost32`), because a 64-bit app can't load them. Add them with **Add Visualization…** in the visualizer's
  right-click menu: pick the plug-in's `.dll`, a `.zip` containing it, or its `.msi` installer (unpacked, not
  installed). Nothing is registered with Windows and no admin rights are needed; each one is test-rendered first and
  only added if it draws. Many are on [WMP Goodies](https://sites.google.com/view/wmpgoodies/visualizations); their
  older `.exe` installers are self-extracting archives that 7-Zip opens.

  The helper treats them the way WMP 9 did: WMP 9-style plug-ins get a player object and a (hidden) window before
  they draw, and a preset is always selected. DirectX ones that draw onto that window instead of the DC they're given
  are copied off it. It also runs with Windows' compatibility registry virtualization, as XP-era programs do, because
  some plug-ins open WMP's machine keys for writing just to read them. Still not working: Microsoft's WMP 9 and 10
  promotional visualizations and the ones built on the same engine (Grind, Terminator III, Looney Tunes), which
  present only black frames; PictureViz II, which waits on a folder dialog; Corona and Fruity, whose registration
  fails.

Audio for the visualizers is captured with WASAPI loopback, so they react to everything Windows plays.

## Limits (web players can't be driven like local files)

- Equalizer, balance and video settings are kept and shown in skins, but can't change the browser's audio.
- Volume and mute control the source app's own entry in the Windows volume mixer (e.g. chrome.exe).
- Seeking, shuffle and repeat work when the site supports them through the Windows media controls.
  Fast-forward and rewind seek ±10 seconds.
- There's no video. Skins with a video area but no visualizer area show the visualizer there.

## Files

- Settings: `%LOCALAPPDATA%\WebSkinPlayer\settings.json`
- Skins: `%LOCALAPPDATA%\WebSkinPlayer\Skins`
- Log: `%LOCALAPPDATA%\WebSkinPlayer\log.txt`
- The app itself: `%LOCALAPPDATA%\Programs\WebSkin Player`

## Building

Needs the .NET 10 SDK, and [Inno Setup](https://jrsoftware.org/isinfo.php) (6 or later) for the installer
(`winget install JRSoftware.InnoSetup`).

```
dotnet build src\WebSkinPlayer
.\tools\Build-Release.ps1
```

`Build-Release.ps1` writes `artifacts\WebSkinPlayer-<version>-Setup.exe` and the portable zip. Both are
self-contained, so the target PC needs no .NET install.

**Releasing:** set `<Version>` in `src\WebSkinPlayer\WebSkinPlayer.csproj`, then push a matching tag:

```
git tag v0.2.0
git push origin v0.2.0
```

The **Build** workflow (`.github/workflows/build.yml`) builds every push and pull request. For a `v*` tag it also
publishes a GitHub release with the installer and zip attached. Builds made by the workflow know their repository and
check it for updates; local builds don't.

### Developer hooks

- `WebSkinPlayer.exe --test-skins <folder|file> <outDir>` loads every skin headlessly. For each skin it writes
  `<skin>.png` (the Skin Chooser preview) and `<skin>.tree.txt` (the element tree), plus a `report.txt` that
  lists script errors.
- `WebSkinPlayer.exe --test-live <folder|file> <outDir> [seconds]` opens each skin for real but hidden and sandboxed.
  Scripts, timers, GIFs, and player events all run, but nothing reaches your player, volume, browser, or settings.
  A made-up song plays (Glue by Bicep, moving on from 0:40), so skins that draw the time from its position show it.
  Afterwards it measures how much of every visualizer area is actually visible, and for a covered one names what is
  drawn over it (`live-report.txt`).
- `WebSkinPlayer.exe --automate script.txt` replays commands through the real input code:
  `click x y`, `drag x1 y1 x2 y2`, `eval js`, `capture file.png`, `capturefull file.png`, `vis Bars 0`,
  `choose Bars 0` (as picked from the right-click menu), `viscolor Bars palette:Fire|hue:120|custom:#RRGGBB` (no
  spec for the original colours), `update` (Install Now on an available update, without asking), `skin path`,
  `widget`, `restore`, `fullmode`,
  `tab skins|online`, `uiclick ButtonName`, `uiselect ListName text`, `state label`, `wait ms`, `exit`.
  `record skin|full|saver|widget <frames> <ms> <scale> <dir>` saves numbered PNG frames of the skin, full mode, the
  screen saver or the taskbar widget (scale 1 = one pixel per skin pixel). Captures render at 96 DPI.
- `--debug-input` logs every press and what it hit. `--widget` starts collapsed into the taskbar widget.
  `--exit` closes a running copy (the installer uses it).
- `WEBSKINPLAYER_DATA=<folder>` runs with a separate data folder, e.g. to try a first run on a clean profile.
  `WEBSKINPLAYER_TESTSIGNAL=1` feeds made-up music to the visualizers. `WEBSKINPLAYER_UPDATES=<url>` reads the
  update check from a GitHub `releases/latest`-shaped JSON there instead (works in local builds too). The installer's
  `/restart=1` starts the player once it's done, as the updater asks.
- `WebSkinPlayer.exe --import-vis <dll|zip|msi|folder> [label]` adds visualization plug-ins and writes
  `Visualizations\import-report.txt`. Automation also has `saver start|stop|next|show <id>|?` and `capturesaver file.png`.
- `WebSkinPlayer.VisHost32.exe probe <dll>` describes a plug-in DLL; `snapshot <dll> <clsid> <out.bmp> [preset]
  [dc|attach|windowed]` test-renders one and reports how much appeared in the DC (`lit`) and on its window
  (`windowLit`). `WEBSKINPLAYER_VISHOST=<exe>` points the player at a development build of the helper.
- `spikes\SmtcTestSource [seconds] [volume] [songEvery] [playlist.txt]` is a fake media session. Volume 0 is silent.
  `songEvery` changes track the ways Chrome does at SoundCloud song changes (a 0.2 s pause, 1.2 s from the fifth
  change on while the next song loads, or the session dropping for 1.5 s, 8 s every fourth change); the third change
  is a real 3 s pause. Next pauses 1.2 s before the next song, as a loading skip does. The playlist has one `Title|Artist|Album|cover.png` line per track. Pin it as the Source (settings `PreferredApp`
  set to its exe name) so the player follows it instead of your browser.

### README pictures

`docs\images` is made by `tools\ReadmeArt`: `art` draws the banner and buttons, `pack` piles skin shots onto the
desktop, and `compose` lays out scenes and encodes the GIFs (a palette per frame, only changed pixels stored). The
skin shots and frames come from the player itself, with a profile holding the skins to show (`WEBSKINPLAYER_DATA`),
`WEBSKINPLAYER_TESTSIGNAL=1`, `SmtcTestSource` playing a playlist as the pinned Source, and an `--automate` script of
`skin`, `choose`, `capture` and `record` lines.

### Layout (`src\WebSkinPlayer`)

- `Media\`: the Windows media session bridge, per-app volume, and the WMP-style player model.
- `Audio\`: loopback capture and FFT, producing WMP's `TimedLevel`-shaped frames.
- `Visualization\`: built-in effects, the hosted `IWMPEffects` adapter, the catalog, and the renderer.
- `Skins\`: the package/resource/parser layers, the object model, `Scripting\` (Jint bridge and JScript
  compatibility), `Elements\`, the views, windows and themes, and the archive.org skin downloader.
- `UI\`: the full-mode window and the taskbar widget.
- `src\WebSkinPlayer.VisHost32`: the 32-bit plug-in helper (shared-memory protocol in `Serve.cs`, mirrored by
  `Visualization\Hosted32Visualization.cs`).
- `installer\`: the Inno Setup script. `tools\`: the release build and icon scripts, and `ReadmeArt` (the README
  pictures in `docs\images`).
- `spikes\`: throwaway experiments. `SmtcTestSource` is a fake media session with a quiet tone, used for tests.

## Credits

Skins are the work of their original authors and aren't part of this repository. The screenshots show them running
in WebSkin Player with a stand-in playlist of public-domain classical titles. Uses
[Jint](https://github.com/sebastienros/jint) (BSD-2-Clause) for JScript and
[NAudio](https://github.com/naudio/NAudio) (MIT) for audio capture.

<p align="center">
  <img src="docs/images/button-dotnet.png" width="88" height="31" alt="Powered by .NET 10">
  <a href="https://github.com/sebastienros/jint"><img src="docs/images/button-jint.png" width="88" height="31" alt="Scripts by Jint"></a>
  <a href="https://github.com/naudio/NAudio"><img src="docs/images/button-naudio.png" width="88" height="31" alt="Audio by NAudio"></a>
  <img src="docs/images/button-1024.png" width="88" height="31" alt="Best viewed at 1024x768">
</p>
