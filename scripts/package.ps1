<#
.SYNOPSIS
  Build a distributable Skreenerino release and collect runtime DLLs.
.DESCRIPTION
  Builds the CLI with the default feature set in release mode and copies the binary
  plus required MinGW runtime DLLs into dist/ so it can run without a toolchain.
#>
param(
  [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
  $env:CARGO_HOME = Join-Path $root ".cargo-home"
  $mingwBin = Join-Path $root ".tools/mingw/mingw64/bin"
  if (Test-Path $mingwBin) { $env:PATH = "$mingwBin;$env:PATH" }
  $env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"

  $cargo = Join-Path $env:USERPROFILE ".cargo/bin/cargo.exe"
  if (-not (Test-Path $cargo)) { $cargo = "cargo" }

  & $cargo build --release -p skreenerino
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

  $dist = Join-Path $root "dist"
  New-Item -ItemType Directory -Force -Path $dist | Out-Null
  $outDir = Join-Path $root ".cargo-target/release"
  Copy-Item (Join-Path $outDir "skreenerino.exe") $dist -Force

  if (Test-Path $mingwBin) {
    foreach ($dll in @("libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll")) {
      $src = Join-Path $mingwBin $dll
      if (Test-Path $src) { Copy-Item $src $dist -Force }
    }
  }

  Copy-Item (Join-Path $root "README.md") $dist -Force -ErrorAction SilentlyContinue
  Write-Host "Packaged into $dist"
} finally {
  Pop-Location
}
