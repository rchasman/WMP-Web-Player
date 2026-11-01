# Renders each skin in WMP and captures the screen over black, white, then black again (an
# animation shows as a difference between the two blacks), and lists the skin windows.
param([string]$Skins, [string]$Out, [int]$Shard = 0, [int]$Shards = 1)
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
New-Item -ItemType Directory -Force $Out | Out-Null
$backdrop = Start-Backdrop
$all = Get-ChildItem $Skins -Filter *.wmz | Sort-Object Name
foreach ($skin in ($all | Where-Object { [array]::IndexOf($all, $_) % $Shards -eq $Shard })) {
    $name = $skin.BaseName
    Set-Backdrop 'black'
    if (-not (Open-Skin $skin.FullName 20)) { "$name : no View Now"; continue }
    $screen = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    Set-Backdrop 'white'; Set-Backdrop 'black'; Capture $screen "$Out\$name.black.png"
    Set-Backdrop 'white'; Capture $screen "$Out\$name.white.png"
    Set-Backdrop 'black'; Capture $screen "$Out\$name.black2.png"
    $k = 0
    foreach ($w in Skin-Windows) {
        $r = $w.Current.BoundingRectangle
        "$name`tw$k`t$([int]$r.X),$([int]$r.Y),$([int]$r.Width),$([int]$r.Height)`t$($w.Current.ClassName)`t$($w.Current.Name)" | Add-Content "$Out\windows.tsv"
        $k++
    }
    "$name : $k windows"
}
Stop-Process -Name wmplayer -Force -ErrorAction SilentlyContinue
Stop-Process -Id $backdrop.Id -Force -ErrorAction SilentlyContinue
