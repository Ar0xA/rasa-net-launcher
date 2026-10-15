# Generates the reference manifest from a known-good Tabula Rasa client install.
# Usage: .\New-Manifest.ps1 -GameDir "C:\...\Tabula Rasa 1.16.5.0" -Out ..\src-tauri\manifest.json
param(
    [Parameter(Mandatory)] [string] $GameDir,
    [string] $Out = (Join-Path $PSScriptRoot '..\src-tauri\manifest.json'),
    [string] $Version = '1.16.5.0',
    # Per-user / runtime-generated files: never verified.
    [string[]] $Exclude = @(
        'ScreenShots/*', '*.log', 'imgui.ini', 'crashreport.xml',
        'client.cfg', 'tabula_rasa.cfg', 'login.cfg', 'LauncherID.ini'
    )
)

$root = (Resolve-Path $GameDir).Path.TrimEnd('\') + '\'
$files = Get-ChildItem -LiteralPath $root -Recurse -File | Where-Object {
    $rel = $_.FullName.Substring($root.Length).Replace('\', '/')
    -not ($Exclude | Where-Object { $rel -like $_ })
}
$i = 0
$entries = foreach ($f in $files) {
    $i++
    Write-Progress -Activity 'Hashing' -Status $f.Name -PercentComplete ($i * 100 / $files.Count)
    [ordered]@{
        path   = $f.FullName.Substring($root.Length).Replace('\', '/')
        size   = $f.Length
        sha256 = (Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash.ToLower()
    }
}

$manifest = [ordered]@{ version = $Version; files = @($entries | Sort-Object { $_.path }) }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
$manifest | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $Out -Encoding utf8
Write-Host "Wrote $($entries.Count) entries to $Out"
