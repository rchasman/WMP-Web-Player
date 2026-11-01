# Runs each test skin in probes/ in WMP. A probe's probe.txt says which view to find and what to
# do: `songs=N` plays N tones first, `clicks=` clicks elements and reads a marker before and after,
# `sample=gifs` times the GIFs it shows. Every probe dumps its elements and captures its window
# over black and over white.
param([string]$Out, [string]$Only = '', [string]$Probe = '')
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
if (-not $Probe) {
    New-Item -ItemType Directory -Force $Out | Out-Null
    $backdrop = Start-Backdrop
    $needsAudio = Get-ChildItem "$PSScriptRoot\probes\*\probe.txt" | Where-Object { (Get-Content $_) -match '^songs=' }
    if ($needsAudio) { Install-VBCable }
    foreach ($dir in Get-ChildItem "$PSScriptRoot\probes" -Directory | Where-Object { -not $Only -or $Only.Split(',') -contains $_.Name }) {
        $wmz = "$env:RUNNER_TEMP\$($dir.Name).zip"
        Compress-Archive -Path (Get-ChildItem $dir.FullName -Exclude probe.txt).FullName -DestinationPath $wmz -Force
        Move-Item $wmz "$env:RUNNER_TEMP\$($dir.Name).wmz" -Force
        "== $($dir.Name)"
        Set-Backdrop 'black'
        Start-Skin "$env:RUNNER_TEMP\$($dir.Name).wmz"
        Invoke-Child @('-Out', $Out, '-Probe', $dir.Name)
    }
    Stop-Process -Name wmplayer -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $backdrop.Id -Force -ErrorAction SilentlyContinue
    exit
}
$name = $Probe; $o = "$Out\$name"; New-Item -ItemType Directory -Force $o | Out-Null
$cfg = @{}; foreach ($line in Get-Content "$PSScriptRoot\probes\$name\probe.txt") { $k, $v = $line.Split('=', 2); $cfg[$k.Trim()] = $v.Trim() }
if (-not (Show-Skin 6)) { 'no View Now'; Capture ([Windows.Forms.Screen]::PrimaryScreen.Bounds) "$o\screen.png"; exit }
if ($cfg.songs) {
    $files = foreach ($song in @('Glue', 'Opal', 'Atlas', 'Gym', 'Late night')[0..([int]$cfg.songs - 1)]) { $p = "$env:RUNNER_TEMP\$song.wav"; New-Tone $p 40; $p }
    ($files -join "`r`n") | Set-Content "$env:RUNNER_TEMP\$name.m3u"
    Start-Process $Exe -ArgumentList "`"$env:RUNNER_TEMP\$name.m3u`""
    Start-Sleep 10
}
$win = Skin-Windows | Where-Object { $_.Current.Name -eq $cfg.view } | Select-Object -First 1
if (-not $win) {
    "no window $($cfg.view); WMP shows: " + ((Skin-Windows | ForEach-Object { "'$($_.Current.Name)'" }) -join ', ')
    Capture ([Windows.Forms.Screen]::PrimaryScreen.Bounds) "$o\screen.png"
    exit
}
$r = $win.Current.BoundingRectangle
"window $r"
$els = @{}
$win.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) | ForEach-Object {
    $c = $_.Current; $b = $c.BoundingRectangle
    if ($c.Name) { $els[$c.Name] = $b }
    "$($c.ControlType.ProgrammaticName)`t$($c.Name)`t$([int]($b.X - $r.X)),$([int]($b.Y - $r.Y)),$([int]$b.Width),$([int]$b.Height)`t$($c.IsOffscreen)"
} | Set-Content "$o\elements.tsv"
Set-Backdrop 'black'; Capture $r "$o\black.png"
Set-Backdrop 'white'; Capture $r "$o\white.png"
Set-Backdrop 'black'
# clicks=target:marker,... clicks the target's centre (or target@x;y inside it) and reports the
# marker's colour before and after; marker * counts the window pixels that changed.
foreach ($pair in ($cfg.clicks -split ',' | Where-Object { $_ })) {
    $target, $marker = $pair.Split(':')
    $at = $null
    if ($target -match '^(\w+)@(\d+);(\d+)$') { $target = $Matches[1]; $at = @([int]$Matches[2], [int]$Matches[3]) }
    $t = $els[$target]
    if (-not $t) { "no element $target"; continue }
    $cx, $cy = if ($at) { ([int]$t.X + $at[0]), ([int]$t.Y + $at[1]) } else { [int]($t.X + $t.Width / 2), [int]($t.Y + $t.Height / 2) }
    if ($marker -eq '*') {
        Capture $r "$o\$target-before.png"; Click $cx $cy; Start-Sleep 1; Capture $r "$o\$target-after.png"
        $a = [Drawing.Bitmap]::FromFile("$o\$target-before.png"); $z = [Drawing.Bitmap]::FromFile("$o\$target-after.png"); $n = 0
        for ($y = 0; $y -lt $a.Height; $y++) { for ($x = 0; $x -lt $a.Width; $x++) { if ($a.GetPixel($x, $y).ToArgb() -ne $z.GetPixel($x, $y).ToArgb()) { $n++ } } }
        $a.Dispose(); $z.Dispose()
        "click $target : $n window pixels changed"
        continue
    }
    $m = $els[$marker]; $mx = [int]($m.X + $m.Width / 2); $my = [int]($m.Y + $m.Height / 2)
    $before = Pixel $mx $my; Click $cx $cy; Start-Sleep 1
    "click $target : $marker $before -> $(Pixel $mx $my)"
}
# sample=gifs: each GIF is a 60x60 square 70 px apart whose frame i has red 20*i; read them as
# fast as the screen allows for 10 s and report the time per frame.
if ($cfg.sample -eq 'gifs') {
    $delays = $cfg.delays.Split(',')
    $rect = [Drawing.Rectangle]::new([int]$r.X, [int]$r.Y, [int]$r.Width, [int]$r.Height)
    $bmp = New-Object Drawing.Bitmap $rect.Width, $rect.Height; $g = [Drawing.Graphics]::FromImage($bmp)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $rows = while ($sw.ElapsedMilliseconds -lt 10000) {
        $g.CopyFromScreen($rect.Location, [Drawing.Point]::Empty, $rect.Size)
        , (@($sw.Elapsed.TotalMilliseconds) + @(for ($k = 0; $k -lt $delays.Count; $k++) { [Math]::Round($bmp.GetPixel(40 + 70 * $k, 40).R / 20) }))
    }
    $g.Dispose(); $bmp.Dispose()
    for ($k = 0; $k -lt $delays.Count; $k++) {
        $frames = 0
        for ($i = 1; $i -lt $rows.Count; $i++) { $frames += (($rows[$i][$k + 1] - $rows[$i - 1][$k + 1]) % 12 + 12) % 12 }
        "gif $($delays[$k]) ms: $([Math]::Round(($rows[-1][0] - $rows[0][0]) / [Math]::Max($frames, 1), 1)) ms per frame"
    }
}
