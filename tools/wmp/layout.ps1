# Loads each skin in WMP and writes every skin element UI Automation reports: skin, view, id,
# type, box relative to its window, offscreen. Compare with the engine's tree dumps.
param([string]$Skins, [string]$Out, [int]$Shard = 0, [int]$Shards = 1, [string]$Skin = '')
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
if (-not $Skin) {
    New-Item -ItemType Directory -Force $Out | Out-Null
    $all = Get-ChildItem $Skins -Filter *.wmz | Sort-Object Name
    foreach ($s in ($all | Where-Object { [array]::IndexOf($all, $_) % $Shards -eq $Shard })) {
        Start-Skin $s.FullName
        Invoke-Child @('-Out', $Out, '-Skin', $s.BaseName)
    }
    Stop-Process -Name wmplayer -Force -ErrorAction SilentlyContinue
    exit
}
$name = $Skin
if (-not (Show-Skin 12)) { "$name : no View Now"; exit }
$n = 0
foreach ($w in Skin-Windows) {
    $c = $w.Current; $wr = $c.BoundingRectangle
    "$name`t$($c.Name)`t`tWINDOW`t$([int]$wr.X),$([int]$wr.Y),$([int]$wr.Width),$([int]$wr.Height)`t$($c.IsOffscreen)" | Add-Content "$Out\layout.tsv"
    foreach ($d in $w.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)) {
        $dc = $d.Current
        if (-not $dc.Name) { continue }
        $r = $dc.BoundingRectangle
        $box = if ($r.IsEmpty) { 'empty' } else { "$([int]($r.X - $wr.X)),$([int]($r.Y - $wr.Y)),$([int]$r.Width),$([int]$r.Height)" }
        "$name`t$($c.Name)`t$($dc.Name)`t$($dc.ControlType.ProgrammaticName)`t$box`t$($dc.IsOffscreen)" | Add-Content "$Out\layout.tsv"
        $n++
    }
}
"$name : $n elements"
