param(
    [string]$Destination = "$env:ProgramFiles\sort-it-now"
)

$ErrorActionPreference = "Stop"
$binaryPath = Join-Path $Destination "sort_it_now.exe"
$readmePath = Join-Path $Destination "README.md"

function Test-IsAdministrator {
    $currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($currentIdentity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Assert-DestinationWritable {
    param([string]$TargetDirectory)

    if (-not (Test-Path $TargetDirectory)) {
        return
    }

    try {
        $probeFile = Join-Path $TargetDirectory ".sort-it-now-write-test-$([guid]::NewGuid())"
        New-Item -ItemType File -Path $probeFile -Force | Out-Null
        Remove-Item -Path $probeFile -Force
    }
    catch {
        $fullDestination = [System.IO.Path]::GetFullPath($TargetDirectory)
        $programFilesRoot = [System.IO.Path]::GetFullPath($env:ProgramFiles)
        $suggestedDestination = Join-Path $env:LOCALAPPDATA "Programs\sort-it-now"

        if ($fullDestination.StartsWith($programFilesRoot, [System.StringComparison]::OrdinalIgnoreCase) -and -not (Test-IsAdministrator)) {
            throw "Cannot remove files from $TargetDirectory. Re-run PowerShell as Administrator or pass -Destination `"$suggestedDestination`" if you installed it per-user."
        }

        throw "Cannot remove files from $TargetDirectory. Re-run PowerShell with sufficient permissions or pass a writable -Destination."
    }
}

function Normalize-PathEntry {
    param([string]$PathEntry)

    if ([string]::IsNullOrWhiteSpace($PathEntry)) {
        return $null
    }

    $normalized = $PathEntry.Trim()
    try {
        $normalized = [System.IO.Path]::GetFullPath($normalized)
        if (Test-Path -LiteralPath $normalized) {
            $normalized = (Get-Item -LiteralPath $normalized -ErrorAction Stop).FullName
        }
    }
    catch {
        # Fall back to the original entry if the path cannot be resolved canonically.
    }

    if ($normalized.Length -gt 3) {
        $normalized = $normalized.TrimEnd(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.IO.Path]::AltDirectorySeparatorChar
        )
    }

    return $normalized
}

if (-not (Test-Path $binaryPath)) {
    Write-Host "sort-it-now is not installed in $Destination."
    exit 0
}

Assert-DestinationWritable -TargetDirectory $Destination

Remove-Item -Path $binaryPath -Force
if (Test-Path $readmePath) {
    Remove-Item -Path $readmePath -Force
}

$pathEntries = ([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ })
$normalizedDestination = Normalize-PathEntry -PathEntry $Destination
$remaining = $pathEntries | Where-Object {
    (Normalize-PathEntry -PathEntry $_) -ne $normalizedDestination
}
[Environment]::SetEnvironmentVariable('Path', ($remaining -join ';'), 'User')

if (Test-Path $Destination) {
    $children = Get-ChildItem -Path $Destination -Force -ErrorAction SilentlyContinue
    if (-not $children) {
        Remove-Item -Path $Destination -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "sort-it-now was successfully uninstalled."
