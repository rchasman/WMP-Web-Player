# Shared by the WMP scripts: start WMP on a skin, find its windows, click, read pixels, capture.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type -Namespace Wmp -Name Mouse -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, System.IntPtr extra);
'@
$Root = [Windows.Automation.AutomationElement]::RootElement
$Exe = "$env:ProgramFiles\Windows Media Player\wmplayer.exe"

# Invokes the first control named $label anywhere on the desktop (WMP's "View Now").
function Press($label) {
    $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $label)
    $b = $Root.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
    if ($b) { $b.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke(); return $true }
    $false
}

# Starts WMP on a skin and waits $settle seconds after it shows; false when WMP never offered it.
function Open-Skin($path, $settle) {
    Stop-Process -Name wmplayer -Force -ErrorAction SilentlyContinue; Start-Sleep 2
    Remove-Item "$env:APPDATA\Microsoft\Media Player\Skins\*" -Force -Recurse -ErrorAction SilentlyContinue
    Start-Process $Exe -ArgumentList "/layout:`"$path`""
    $pressed = $false
    for ($t = 0; $t -lt 30 -and -not $pressed; $t++) { Start-Sleep 1; $pressed = Press 'View Now' }
    if ($pressed) { Start-Sleep $settle }
    $pressed
}

# WMP's skin windows: UI Automation elements of class "WMP Skin Host", named by view id.
function Skin-Windows {
    $pids = @(Get-Process wmplayer -ErrorAction SilentlyContinue | ForEach-Object Id)
    $Root.FindAll([Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition) |
        Where-Object { $pids -contains $_.Current.ProcessId -and $_.Current.ClassName -eq 'WMP Skin Host' }
}

function Click($x, $y) {
    [Wmp.Mouse]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 150
    [Wmp.Mouse]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 80
    [Wmp.Mouse]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
}

function Pixel($x, $y) {
    $b = New-Object Drawing.Bitmap 1, 1; $g = [Drawing.Graphics]::FromImage($b)
    $g.CopyFromScreen($x, $y, 0, 0, [Drawing.Size]::new(1, 1)); $c = $b.GetPixel(0, 0); $g.Dispose(); $b.Dispose()
    '#{0:X2}{1:X2}{2:X2}' -f $c.R, $c.G, $c.B
}

function Capture($rect, $path) {
    $b = New-Object Drawing.Bitmap ([int]$rect.Width), ([int]$rect.Height); $g = [Drawing.Graphics]::FromImage($b)
    $g.CopyFromScreen([int]$rect.X, [int]$rect.Y, 0, 0, $b.Size); $b.Save($path); $g.Dispose(); $b.Dispose()
}

# A full-screen window behind WMP whose colour follows Set-Backdrop, so captures over black and
# white tell clear pixels from drawn ones.
function Start-Backdrop {
    $env:BACKDROP_FILE = "$env:RUNNER_TEMP\backdrop.txt"
    'black' | Set-Content $env:BACKDROP_FILE
    Start-Process pwsh -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile', '-File', "$PSScriptRoot\backdrop.ps1"
    Start-Sleep 5
}
function Set-Backdrop($colour) { $colour | Set-Content $env:BACKDROP_FILE; Start-Sleep -Milliseconds 900 }

# The runner has no audio device and WMP won't play without one: VB-Cable gives it one.
function Install-VBCable {
    curl.exe -sSL -o "$env:RUNNER_TEMP\vbcable.zip" https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack43.zip
    Expand-Archive "$env:RUNNER_TEMP\vbcable.zip" "$env:RUNNER_TEMP\vbcable" -Force
    Get-ChildItem "$env:RUNNER_TEMP\vbcable\*.cat" | ForEach-Object {
        $sig = Get-AuthenticodeSignature $_.FullName
        if ($sig.SignerCertificate) {
            $store = New-Object Security.Cryptography.X509Certificates.X509Store 'TrustedPublisher', 'LocalMachine'
            $store.Open('ReadWrite'); $store.Add($sig.SignerCertificate); $store.Close()
        }
    }
    Start-Process "$env:RUNNER_TEMP\vbcable\VBCABLE_Setup_x64.exe" -ArgumentList '-i', '-h' -Wait
    Start-Sleep 10; Restart-Service Audiosrv -Force; Start-Sleep 5
}

# A tone of $seconds as a WAV; WMP names it by its file name.
function New-Tone($path, $seconds) {
    $rate = 22050; $n = $rate * $seconds
    $ms = New-Object IO.MemoryStream; $w = New-Object IO.BinaryWriter $ms
    $w.Write([Text.Encoding]::ASCII.GetBytes('RIFF')); $w.Write([int](36 + 2 * $n)); $w.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '))
    $w.Write([int]16); $w.Write([int16]1); $w.Write([int16]1); $w.Write([int]$rate); $w.Write([int]($rate * 2)); $w.Write([int16]2); $w.Write([int16]16)
    $w.Write([Text.Encoding]::ASCII.GetBytes('data')); $w.Write([int](2 * $n))
    for ($i = 0; $i -lt $n; $i++) { $w.Write([int16](3000 * [Math]::Sin(2 * [Math]::PI * 330 * $i / $rate))) }
    [IO.File]::WriteAllBytes($path, $ms.ToArray())
}
