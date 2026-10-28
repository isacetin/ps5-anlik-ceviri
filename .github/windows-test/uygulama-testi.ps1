# Windows testi: masaustu uygulamasi PC oyunu modunda, gercek klavye ve fare girdisiyle.
# Test penceresi (oyun-penceresi.ps1) acik olmali: (40,40), 900x560, baslik "Oyun Testi".
param(
    [Parameter(Mandatory)] [string]$Exe,
    [Parameter(Mandatory)] [string]$Out,
    [Parameter(Mandatory)] [string]$DataDir,
    [bool]$Translate = $true
)
$ErrorActionPreference = "Stop"
. "$PSScriptRoot\yardimci.ps1"

$failures = New-Object System.Collections.Generic.List[string]
function Check([bool]$ok, [string]$message) {
    if ($ok) { Write-Host "OK   $message" } else { Write-Host "HATA $message"; $failures.Add($message) }
}

New-Item -ItemType Directory -Force -Path $Out, $DataDir | Out-Null
# Kurulum bitmis, PC oyunu secili. BOM'suz UTF-8 (serde_json BOM'u kabul etmez).
$settings = @{
    source = "pcGame"; window = "Oyun Testi"; output = "screen"; translate = $Translate; engine = "local"
    fps = 2; setupDone = $true
} | ConvertTo-Json
[System.IO.File]::WriteAllText("$DataDir\settings.json", $settings)

$env:CEVIRI_DATA_DIR = $DataDir
$env:CEVIRI_AUTOSTART = "1"
$app = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 15
$app.Refresh()
Check (-not $app.HasExited) "uygulama acildi"
if ($app.MainWindowHandle -ne [IntPtr]::Zero) { Minimize-Window $app.MainWindowHandle }

# Oyunu oyuncu gibi tiklayarak one al; ceviri bir sure calissin.
Click-At 600 160
Start-Sleep -Seconds 12
Save-Screen "$Out\1-calisiyor.png"
$title = Get-ForegroundTitle
Check ($title -eq "Oyun Testi") "ceviri calisirken oyun ondeki pencere (ondeki: '$title')"

Send-Keys "^+t"
Start-Sleep -Seconds 3
Save-Screen "$Out\2-gizli.png"
Send-Keys "^+t"
Start-Sleep -Seconds 3
Save-Screen "$Out\3-geri-geldi.png"
$title = Get-ForegroundTitle
Check ($title -eq "Oyun Testi") "Ctrl+Shift+T sonrasi oyun ondeki pencere (ondeki: '$title')"

Send-Keys "^+r"
Start-Sleep -Seconds 5
Save-Screen "$Out\4-bolge-penceresi.png"
$title = Get-ForegroundTitle
Check ($title -like "*b?lge se?*") "Ctrl+Shift+R bolge penceresini acti (ondeki: '$title')"

# Altyazi bandini ciz.
Drag-Mouse 70 480 915 585
Start-Sleep -Seconds 8
Save-Screen "$Out\5-bolge-secildi.png"
$title = Get-ForegroundTitle
Check ($title -eq "Oyun Testi") "bolge secildikten sonra odak oyunda (ondeki: '$title')"

$saved = Get-Content "$DataDir\settings.json" -Raw | ConvertFrom-Json
$region = $saved.regions.'oyun testi'
Check ($null -ne $region) "bolge ayarlara kaydedildi"
if ($region) { Write-Host ("Bolge: x={0:N3} y={1:N3} g={2:N3} y={3:N3}" -f $region.x, $region.y, $region.width, $region.height) }

$app.Refresh()
Check (-not $app.HasExited) "uygulama hala calisiyor"
Copy-Item "$DataDir\settings.json" "$Out\settings.json"
Get-ChildItem $DataDir -Filter "translations*.json" | Copy-Item -Destination $Out
Stop-Process -Id $app.Id -ErrorAction SilentlyContinue

if ($failures.Count -gt 0) {
    Write-Host "Basarisiz: $($failures -join '; ')"
    exit 1
}
