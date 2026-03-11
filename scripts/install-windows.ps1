param(
    [string]$Destination = "$env:ProgramFiles\sort-it-now"
)

$ErrorActionPreference = "Stop"
$Owner = if ($env:SORT_IT_NOW_GITHUB_OWNER) { $env:SORT_IT_NOW_GITHUB_OWNER } else { "JosunLP" }
$Repo = if ($env:SORT_IT_NOW_GITHUB_REPO) { $env:SORT_IT_NOW_GITHUB_REPO } else { "sort-it-now" }
$RequestedVersion = if ($env:SORT_IT_NOW_VERSION) { $env:SORT_IT_NOW_VERSION } else { "latest" }
$scriptDir = if ($MyInvocation.MyCommand.Path) { Split-Path -Parent $MyInvocation.MyCommand.Path } else { (Get-Location).Path }
$binaryPath = Join-Path $scriptDir "sort_it_now.exe"

function Add-DestinationToPath {
    param([string]$PathEntry)

    $path = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($path)) {
        $entries = $path -split ';' | Where-Object { $_ }
    }

    if ($entries -contains $PathEntry) {
        return
    }

    $entries += $PathEntry
    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
    Write-Host "The installation directory was added to user PATH. You may need to open a new terminal."
}

function Install-LocalBinary {
    param(
        [string]$BinaryPath,
        [string]$TargetDirectory,
        [string]$ReadmeSource
    )

    if (-not (Test-Path $TargetDirectory)) {
        New-Item -ItemType Directory -Path $TargetDirectory -Force | Out-Null
    }

    Copy-Item -Path $BinaryPath -Destination (Join-Path $TargetDirectory "sort_it_now.exe") -Force
    Copy-Item -Path $ReadmeSource -Destination (Join-Path $TargetDirectory "README.md") -Force -ErrorAction SilentlyContinue
    Write-Host "sort-it-now was installed to $TargetDirectory."
    Add-DestinationToPath -PathEntry $TargetDirectory
    Write-Host "Start the service with: sort_it_now.exe"
}

function Get-ReleaseAsset {
    param(
        [object]$Release,
        [string]$Suffix
    )

    $archive = $Release.assets | Where-Object { $_.name -like "*$Suffix.zip" } | Select-Object -First 1
    $checksum = $Release.assets | Where-Object { $_.name -like "*$Suffix.zip.sha256" } | Select-Object -First 1
    if (-not $archive) {
        throw "Could not find a release archive for $Suffix."
    }

    return @{
        Archive = $archive
        Checksum = $checksum
    }
}

function Install-FromRelease {
    param([string]$TargetDirectory)

    $headers = @{ Accept = "application/vnd.github+json" }
    if ($env:SORT_IT_NOW_GITHUB_TOKEN) {
        $headers["Authorization"] = "Bearer $($env:SORT_IT_NOW_GITHUB_TOKEN)"
    }
    elseif ($env:GITHUB_TOKEN) {
        $headers["Authorization"] = "Bearer $($env:GITHUB_TOKEN)"
    }

    $releaseUrl = if ($RequestedVersion -eq "latest") {
        "https://api.github.com/repos/$Owner/$Repo/releases/latest"
    }
    else {
        "https://api.github.com/repos/$Owner/$Repo/releases/tags/$RequestedVersion"
    }

    $release = Invoke-RestMethod -Uri $releaseUrl -Headers $headers
    $assetSet = Get-ReleaseAsset -Release $release -Suffix "windows-x86_64"

    $tempDir = Join-Path $env:TEMP "sort-it-now-install-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

    try {
        $archivePath = Join-Path $tempDir "release.zip"
        $checksumPath = Join-Path $tempDir "release.zip.sha256"

        Write-Host "Downloading sort-it-now release..."
        Invoke-WebRequest -Uri $assetSet.Archive.browser_download_url -Headers $headers -OutFile $archivePath
        if ($assetSet.Checksum) {
            Invoke-WebRequest -Uri $assetSet.Checksum.browser_download_url -Headers $headers -OutFile $checksumPath
            $expectedHash = ((Get-Content -Path $checksumPath -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
            $actualHash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($expectedHash -ne $actualHash) {
                throw "Checksum verification failed for the downloaded archive."
            }
        }

        Expand-Archive -Path $archivePath -DestinationPath $tempDir -Force
        $bundleDir = Get-ChildItem -Path $tempDir -Directory -Filter "sort-it-now-*" | Select-Object -First 1
        if (-not $bundleDir) {
            throw "Could not find the extracted release bundle."
        }

        & (Join-Path $bundleDir.FullName "install.ps1") -Destination $TargetDirectory
    }
    finally {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if (Test-Path $binaryPath) {
    Install-LocalBinary -BinaryPath $binaryPath -TargetDirectory $Destination -ReadmeSource (Join-Path $scriptDir "README.md")
}
else {
    Install-FromRelease -TargetDirectory $Destination
}
