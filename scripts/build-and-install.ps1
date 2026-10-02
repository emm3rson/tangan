<#
.SYNOPSIS
  Build Toolbox and optionally bump version and launch NSIS installer.

.DESCRIPTION
  Windows-first helper for `utility-desktop` (Toolbox). Runs full validation
  (typecheck/test/cargo) then `tauri build` and launches the NSIS installer.
  Optional version bump supports both explicit --Version and semantic --Bump.

.PARAMETER Version
  Explicit version to set, e.g. "0.3.0". Must be semver X.Y.Z.

.PARAMETER Bump
  Semantic bump to apply to current version in package.json: patch|minor|major.
  patch: 0.5.0 -> 0.5.1, minor: 0.5.0 -> 0.6.0, major: 0.5.0 -> 1.0.0.
  Mutually exclusive with -Version. If neither is passed, version is unchanged.

.PARAMETER NoInstall
  Skip launching installer after build.

.PARAMETER SkipChecks
  Skip validation (typecheck/test/cargo). Useful for quick iteration.

.PARAMETER NoBuild
  Only bump version, don't build. Useful for `ps1 -Bump patch -NoBuild`.

.EXAMPLE
  ./scripts/build-and-install.ps1
  # full checks + build + install NSIS exe

.EXAMPLE
  ./scripts/build-and-install.ps1 -Bump patch
  # bump 0.5.0 -> 0.5.1, then build + install

.EXAMPLE
  ./scripts/build-and-install.ps1 -Version 0.3.0 -SkipChecks -NoInstall
  # only bump to 0.3.0 and build, no validation, no installer

.EXAMPLE
  npm run build:install -- -Bump minor
#>
[CmdletBinding(DefaultParameterSetName = "Default")]
param(
  [Parameter(ParameterSetName = "Explicit")]
  [ValidatePattern("^\d+\.\d+\.\d+$")]
  [string]$Version,

  [Parameter(ParameterSetName = "Bump")]
  [ValidateSet("patch", "minor", "major")]
  [string]$Bump,

  [switch]$NoInstall,

  [switch]$SkipChecks,

  [switch]$NoBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
# Initialize LASTEXITCODE for StrictMode (not set until external command runs)
if (-not (Test-Path variable:global:LASTEXITCODE)) { $global:LASTEXITCODE = 0 }

# Resolve repo root (script is in <root>/scripts)
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $RepoRoot

function Write-Step([string]$msg) {
  Write-Host "`n== $msg ==" -ForegroundColor Cyan
}

function Get-CurrentVersion {
  $pkg = Get-Content (Join-Path $RepoRoot "package.json") -Raw | ConvertFrom-Json
  return $pkg.version
}

function Invoke-Bump([string]$current, [string]$kind) {
  $parts = $current.Split(".")
  [int]$maj = $parts[0]; [int]$min = $parts[1]; [int]$pat = $parts[2]
  switch ($kind) {
    "patch" { $pat++ }
    "minor" { $min++; $pat = 0 }
    "major" { $maj++; $min = 0; $pat = 0 }
  }
  return "$maj.$min.$pat"
}

function Set-Version([string]$newVersion) {
  Write-Step "Bumping version to $newVersion"

  $versionPaths = @(
    "package.json", "package-lock.json", "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock", "src-tauri/tauri.conf.json"
  ) | ForEach-Object { Join-Path $RepoRoot $_ }
  $originalContents = @{}
  foreach ($versionPath in $versionPaths) {
    $originalContents[$versionPath] = [System.IO.File]::ReadAllBytes($versionPath)
  }

  try {
    # Keep both npm manifests synchronized without install scripts or a Git tag.
    npm version $newVersion --no-git-tag-version --ignore-scripts --allow-same-version
    if ($LASTEXITCODE -ne 0) { throw "npm version update failed" }

    # src-tauri/Cargo.toml - first occurrence of `version = "x.y.z"` under [package]
    $cargoPath = Join-Path $RepoRoot "src-tauri/Cargo.toml"
    $cargoRaw = Get-Content $cargoPath -Raw
    $cargoRaw = $cargoRaw -replace '(?m)^version\s*=\s*"\d+\.\d+\.\d+"', "version = `"$newVersion`""
    $cargoRaw = $cargoRaw.TrimEnd("`r","`n") + "`r`n"
    Set-Content -Path $cargoPath -Value $cargoRaw -Encoding UTF8 -NoNewline

    # src-tauri/tauri.conf.json
    $tauriPath = Join-Path $RepoRoot "src-tauri/tauri.conf.json"
    $tauriRaw = Get-Content $tauriPath -Raw
    $tauriRaw = $tauriRaw -replace '"version"\s*:\s*"\d+\.\d+\.\d+"', "`"version`": `"$newVersion`""
    $tauriRaw = $tauriRaw.TrimEnd("`r","`n") + "`r`n"
    Set-Content -Path $tauriPath -Value $tauriRaw -Encoding UTF8 -NoNewline

    Write-Host "  updated npm manifests, src-tauri/Cargo.toml, src-tauri/tauri.conf.json -> $newVersion" -ForegroundColor Green

    # Keep Cargo.lock in sync so --locked checks don't fail after a bump
    Write-Host "  updating Cargo.lock..." -ForegroundColor DarkGray
    cargo update --manifest-path (Join-Path $RepoRoot "src-tauri/Cargo.toml") --workspace --offline
    if ($LASTEXITCODE -ne 0) { throw "Cargo.lock version update failed" }
  } catch {
    foreach ($versionPath in $versionPaths) {
      [System.IO.File]::WriteAllBytes($versionPath, $originalContents[$versionPath])
    }
    throw
  }
}

# --- Version handling ---
$current = Get-CurrentVersion
$newVersion = $null
if ($PSCmdlet.ParameterSetName -eq "Bump" -and $Bump) {
  $newVersion = Invoke-Bump $current $Bump
  Write-Host "Current $current -> $Bump -> $newVersion" -ForegroundColor Yellow
  Set-Version $newVersion
} elseif ($PSCmdlet.ParameterSetName -eq "Explicit" -and $Version) {
  $newVersion = $Version
  Write-Host "Current $current -> explicit -> $newVersion" -ForegroundColor Yellow
  Set-Version $newVersion
} else {
  Write-Host "Version $current (unchanged)" -ForegroundColor DarkGray
}

if ($NoBuild) {
  Write-Host "NoBuild set - stopping after version bump." -ForegroundColor Yellow
  exit 0
}

# --- Validation (full checks) ---
if (-not $SkipChecks) {
  Write-Step "Full validation"

  Write-Host "  npm run typecheck..." -ForegroundColor DarkGray
  npm run typecheck
  if ($LASTEXITCODE -ne 0) { throw "typecheck failed" }

  Write-Host "  npm run lint..." -ForegroundColor DarkGray
  npm run lint
  if ($LASTEXITCODE -ne 0) { throw "lint failed" }

  Write-Host "  npm run test..." -ForegroundColor DarkGray
  npm run test
  if ($LASTEXITCODE -ne 0) { throw "tests failed" }

  Write-Host "  cargo fmt --check..." -ForegroundColor DarkGray
  cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
  if ($LASTEXITCODE -ne 0) { throw "cargo fmt check failed" }

  Write-Host "  cargo clippy..." -ForegroundColor DarkGray
  cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
  if ($LASTEXITCODE -ne 0) { throw "clippy failed" }

  Write-Host "  cargo test..." -ForegroundColor DarkGray
  cargo test --manifest-path src-tauri/Cargo.toml
  if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

  Write-Step "Validation passed"
} else {
  Write-Host 'Skipping validation (--SkipChecks)' -ForegroundColor Yellow
}

# --- Tauri bundle ---
# Tauri's beforeBuildCommand builds the frontend once.
Write-Step 'Building Tauri bundle (this takes a while)'
npm run tauri -- build
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

# --- Locate NSIS installer ---
$nsisDir = Join-Path $RepoRoot "src-tauri/target/release/bundle/nsis"
$msiDir = Join-Path $RepoRoot "src-tauri/target/release/bundle/msi"
$installer = $null
if (Test-Path $nsisDir) {
  $installer = Get-ChildItem -Path $nsisDir -Filter *.exe -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
}
# fallback to MSI if no NSIS exe found (e.g. if tauri.conf adds msi target)
if (-not $installer -and (Test-Path $msiDir)) {
  $installer = Get-ChildItem -Path $msiDir -Filter *.msi -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
}
if (-not $installer -and -not $NoInstall) {
  Write-Warning "Installer not found in $nsisDir - listing bundle contents:"
  Get-ChildItem -Path (Join-Path $RepoRoot "src-tauri/target/release/bundle") -Recurse -ErrorAction SilentlyContinue | Select-Object FullName | Format-Table
  throw "NSIS installer not found after build"
}

if ($NoInstall) {
  Write-Step 'Build done - installer launch skipped (--NoInstall)'
  if ($installer) { Write-Host "  Installer at: $($installer.FullName)" -ForegroundColor DarkGray }
  exit 0
}

Write-Step "Launching NSIS installer"
Write-Host "  $($installer.FullName)" -ForegroundColor Green
Write-Host "  If UAC prompts, approve to continue." -ForegroundColor Yellow

try {
  Start-Process -FilePath $installer.FullName -Verb Open
  Write-Host "Installer window triggered." -ForegroundColor Green
} catch {
  Write-Warning "Start-Process failed, trying fallback: $_"
  if ($installer.Extension -eq ".msi") {
    Start-Process -FilePath "msiexec.exe" -ArgumentList "/i `"$($installer.FullName)`"" -Verb RunAs
  } else {
    Start-Process -FilePath $installer.FullName -Verb RunAs
  }
}

Write-Host "`nDone. Version $newVersion (if bumped) built and installer launched." -ForegroundColor Cyan
