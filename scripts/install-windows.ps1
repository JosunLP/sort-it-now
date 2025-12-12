param(
    [string]$Destination = "$env:ProgramFiles\sort-it-now"
)

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$binaryPath = Join-Path $scriptDir "sort_it_now.exe"

if (-not (Test-Path $binaryPath)) {
    Write-Error "Die Datei sort_it_now.exe wurde nicht gefunden. Fuehre das Skript im entpackten Release-Ordner aus."
    exit 1
}

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
}

Copy-Item -Path $binaryPath -Destination (Join-Path $Destination "sort_it_now.exe") -Force
Copy-Item -Path (Join-Path $scriptDir "README.md") -Destination (Join-Path $Destination "README.md") -Force -ErrorAction SilentlyContinue

Write-Host "sort-it-now wurde nach $Destination installiert."

$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($path -notlike "*$Destination*") {
    if ([string]::IsNullOrWhiteSpace($path)) {
        $newPath = $Destination
    }
    else {
        $newPath = "$path;$Destination"
    }

    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Das Installationsverzeichnis wurde zum Benutzer-PATH hinzugefuegt. Du musst eventuell ein neues Terminal oeffnen."
}

Write-Host "Starte den Dienst mit: sort_it_now.exe"
