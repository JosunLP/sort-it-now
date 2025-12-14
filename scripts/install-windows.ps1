param(
    [string]$Destination = "$env:ProgramFiles\sort-it-now"
)

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$binaryPath = Join-Path $scriptDir "sort_it_now.exe"

if (-not (Test-Path $binaryPath)) {
    Write-Error "The file sort_it_now.exe was not found. Run this script in the extracted release folder."
    exit 1
}

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
}

Copy-Item -Path $binaryPath -Destination (Join-Path $Destination "sort_it_now.exe") -Force
Copy-Item -Path (Join-Path $scriptDir "README.md") -Destination (Join-Path $Destination "README.md") -Force -ErrorAction SilentlyContinue

Write-Host "sort-it-now was installed to $Destination."

$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($path -notlike "*$Destination*") {
    if ([string]::IsNullOrWhiteSpace($path)) {
        $newPath = $Destination
    }
    else {
        $newPath = "$path;$Destination"
    }

    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "The installation directory was added to user PATH. You may need to open a new terminal."
}

Write-Host "Start the service with: sort_it_now.exe"
