<#
Makes the Windows installer from a release bundle: Signet-<version>-x64-setup.exe (NSIS, per user, like the Electron one).

  powershell -File scripts\package-windows.ps1 [-SkipBundle]

Needs makensis (NSIS; preinstalled on GitHub's Windows runners, else `choco install nsis`). Output: dist\.
The installer is unsigned (no certificate yet). The Windows arm64 build is not packaged (the Electron release has none either).
#>
[CmdletBinding()]
param([switch]$SkipBundle)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Dist = Join-Path $Root 'dist'
$Bundle = Join-Path $Dist 'signet-windows'

$inSection = $false
$Version = '0.5.0'
foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
  if ($line -match '^\[') { $inSection = ($line.Trim() -eq '[workspace.package]') }
  elseif ($inSection -and $line -match '^version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
}

if (-not $SkipBundle) {
  & (Join-Path $PSScriptRoot 'bundle-windows.ps1') -Release
  if ($LASTEXITCODE -ne 0) { throw "bundle-windows.ps1 failed" }
}
if (-not (Test-Path (Join-Path $Bundle 'signet.exe'))) { throw "$Bundle not found (run without -SkipBundle)" }

$makensis = Get-Command makensis.exe -ErrorAction SilentlyContinue
if ($makensis) { $makensis = $makensis.Source }
else {
  $makensis = @("${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:ProgramFiles\NSIS\makensis.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $makensis) { throw 'makensis not found (choco install nsis)' }

# NSIS needs a four-part file version: 1.0.0-beta.1 becomes 1.0.0 there (the full version stays in the registry texts).
$Out = Join-Path $Dist "Signet-$Version-x64-setup.exe"
if (Test-Path $Out) { Remove-Item -Force $Out }
$numeric = ($Version -split '-')[0]
& $makensis /V2 "/DVERSION=$numeric" "/DSOURCE=$Bundle" "/DOUTFILE=$Out" (Join-Path $PSScriptRoot 'windows\installer.nsi')
if ($LASTEXITCODE -ne 0) { throw "makensis failed (exit code $LASTEXITCODE)" }
Write-Host "Package: $Out"
