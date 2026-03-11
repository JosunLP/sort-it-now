param(
    [string]$Destination = "$env:ProgramFiles\sort-it-now"
)

$ErrorActionPreference = "Stop"
$binaryPath = Join-Path $Destination "sort_it_now.exe"
$readmePath = Join-Path $Destination "README.md"

if (-not (Test-Path $binaryPath)) {
    Write-Host "sort-it-now is not installed in $Destination."
    exit 0
}

Remove-Item -Path $binaryPath -Force
if (Test-Path $readmePath) {
    Remove-Item -Path $readmePath -Force
}

$pathEntries = ([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ })
$remaining = $pathEntries | Where-Object { $_ -ne $Destination }
[Environment]::SetEnvironmentVariable('Path', ($remaining -join ';'), 'User')

if (Test-Path $Destination) {
    $children = Get-ChildItem -Path $Destination -Force -ErrorAction SilentlyContinue
    if (-not $children) {
        Remove-Item -Path $Destination -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "sort-it-now was successfully uninstalled."
