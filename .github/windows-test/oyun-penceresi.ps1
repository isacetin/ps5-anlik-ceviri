# Windows testi: oyun yerine gecen pencere. Koyu arka plan, ustte menu, altta altyazi.
# Baslik "Oyun Testi"; uygulama bu pencereyi PC oyunu olarak okur.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

$form = New-Object System.Windows.Forms.Form
$form.Text = "Oyun Testi"
$form.StartPosition = "Manual"
$form.Location = New-Object System.Drawing.Point(40, 40)
$form.Size = New-Object System.Drawing.Size(900, 560)
$form.BackColor = [System.Drawing.Color]::FromArgb(24, 28, 36)

function Add-Text([string]$text, [int]$x, [int]$y, [float]$size) {
    $label = New-Object System.Windows.Forms.Label
    $label.Text = $text
    $label.ForeColor = [System.Drawing.Color]::White
    $label.Font = New-Object System.Drawing.Font("Segoe UI", $size, [System.Drawing.FontStyle]::Bold)
    $label.AutoSize = $true
    $label.Location = New-Object System.Drawing.Point($x, $y)
    $form.Controls.Add($label)
}

Add-Text "MAIN MENU" 40 30 26
Add-Text "Continue" 60 120 20
Add-Text "Options" 60 170 20
Add-Text "Quit Game" 60 220 20
Add-Text "The door is locked. We need to find another way." 90 430 20

[System.Windows.Forms.Application]::Run($form)
