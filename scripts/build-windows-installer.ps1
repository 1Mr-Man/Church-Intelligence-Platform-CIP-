[CmdletBinding()]
param(
    [switch]$SkipWhisper,
    [string]$LibclangPath,
    [string]$CmakeBinPath
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$desktop = Join-Path $repo 'apps\desktop'
$LibclangPath = if ($LibclangPath) { $LibclangPath } else { Join-Path $repo '.tmp\libclang\clang\native' }
$CmakeBinPath = if ($CmakeBinPath) { $CmakeBinPath } else { Join-Path $repo '.tmp\cmake\cmake\data\bin' }
$tauri = Join-Path $desktop 'node_modules\.bin\tauri.cmd'
$bundle = Join-Path $repo 'target\release\bundle\nsis\Church Intelligence Platform_0.1.0_x64-setup.exe'
$releaseDir = Join-Path $repo 'release\windows'
$releaseInstaller = Join-Path $releaseDir 'Church Intelligence Platform_0.1.0_x64-setup.exe'

if (-not (Test-Path -LiteralPath $tauri)) {
    throw "Local Tauri CLI not found at $tauri. Restore dependencies once, then rerun this script."
}

$tauriPackage = Join-Path $desktop 'node_modules\@tauri-apps\cli\tauri.js'
if (-not (Test-Path -LiteralPath $tauriPackage)) {
    throw "Tauri CLI is present but unreadable at $tauriPackage. Restore dependencies and ensure Windows permissions allow Node to read node_modules."
}

if (-not $SkipWhisper) {
    if (-not (Test-Path -LiteralPath (Join-Path $LibclangPath 'libclang.dll'))) {
        throw "libclang.dll not found under $LibclangPath. Install the local build prerequisite or use -SkipWhisper."
    }
    $cmakeExe = Join-Path $CmakeBinPath 'cmake.exe'
    if (-not (Test-Path -LiteralPath $cmakeExe)) {
        throw "cmake.exe not found under $CmakeBinPath. Install the local build prerequisite or use -SkipWhisper."
    }
    $env:LIBCLANG_PATH = (Resolve-Path $LibclangPath).Path
    $env:CMAKE_TOOLCHAIN_FILE = Join-Path $repo 'scripts\whisper-windows-msvc-simd.cmake'
    $env:Path = "$(Resolve-Path $CmakeBinPath).Path;$env:Path"
    $features = @('--features', 'whisper')
} else {
    $features = @()
}

# Call the existing local CLI directly. This avoids pnpm's verify/install step
# and therefore avoids failing on a locked global pnpm store during packaging.
Push-Location $desktop
try {
    & $tauri build --bundles nsis @features
    if ($LASTEXITCODE -ne 0) { throw "Tauri packaging failed with exit code $LASTEXITCODE." }
} finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $bundle)) {
    throw "Expected NSIS artifact was not produced: $bundle"
}

New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
Copy-Item -LiteralPath $bundle -Destination $releaseInstaller -Force
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $releaseInstaller).Hash.ToLowerInvariant()
$size = (Get-Item -LiteralPath $releaseInstaller).Length
$timestamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')

Set-Content -LiteralPath "$releaseInstaller.sha256" -Value $hash -NoNewline
$manifestPath = Join-Path $releaseDir 'release-manifest.json'
if (Test-Path -LiteralPath $manifestPath) {
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $manifest.installer.fileSizeBytes = $size
    $manifest.installer.sha256 = $hash
    $manifest.installer.buildTimestampUtc = $timestamp
    $manifest.installer.packagedIntoReleaseTimestampUtc = $timestamp
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $manifestPath
}

[pscustomobject]@{
    Installer = $releaseInstaller
    SizeBytes = $size
    Sha256 = $hash
    BuildTimestampUtc = $timestamp
} | Format-List
