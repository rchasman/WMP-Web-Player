# A full-screen window behind WMP whose colour follows the word in $env:BACKDROP_FILE.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$f = New-Object System.Windows.Forms.Form
$f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false
$f.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$f.BackColor = [System.Drawing.Color]::Black
$t = New-Object System.Windows.Forms.Timer
$t.Interval = 100
$t.Add_Tick({
    $want = if ((Get-Content $env:BACKDROP_FILE -ErrorAction SilentlyContinue) -eq 'white') { [System.Drawing.Color]::White } else { [System.Drawing.Color]::Black }
    $f.BackColor = $want
    $f.Refresh()
})
$t.Start()
[System.Windows.Forms.Application]::Run($f)
