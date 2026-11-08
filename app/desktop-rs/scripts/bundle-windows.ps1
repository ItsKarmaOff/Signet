<#
Build Signet and lay out dist\signet-windows\ (binary, helper, CEF runtime, manifest, icon). Windows only, untested so far.

  powershell -File scripts\bundle-windows.ps1 [-Release] [-Zip]

Debug is the default. -Zip also writes dist\signet-<version>-windows-<arch>.zip.

The CEF distribution is the one cef-dll-sys downloads during `cargo build` into $env:CEF_PATH
(default: app\desktop-rs\.cache\cef\<cef version>\cef_windows_<arch>). Set CEF_PATH to reuse another cache.

Layout (the DLLs must sit next to the executables):
  signet.exe, signet-helper.exe, signet.exe.manifest (or embedded with mt.exe when the Windows SDK is on the PATH)
  libcef.dll, chrome_elf.dll, d3dcompiler_47.dll, libEGL.dll, libGLESv2.dll, vk_swiftshader.dll, vulkan-1.dll,
  dxcompiler.dll, dxil.dll, vk_swiftshader_icd.json, *.pak, icudtl.dat, v8_context_snapshot.bin, locales\
  signet.ico, signet.png
Packaging: an installer (NSIS, Inno Setup or WiX) takes this folder as is. Signing: signtool sign /fd SHA256 /tr <timestamp url> /td SHA256 /a signet.exe signet-helper.exe libcef.dll.
#>
[CmdletBinding()]
param(
  [switch]$Release,
  [switch]$Zip
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $env:CEF_PATH) { $env:CEF_PATH = Join-Path $Root '.cache\cef' }
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '4' }

$Profile_ = if ($Release) { 'release' } else { 'debug' }
$Dist = Join-Path $Root 'dist'
$Out = Join-Path $Dist 'signet-windows'

function Get-WorkspaceVersion {
  $inSection = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\[') { $inSection = ($line.Trim() -eq '[workspace.package]') }
    elseif ($inSection -and $line -match '^version\s*=\s*"([^"]+)"') { return $Matches[1] }
  }
  return '0.5.0'
}
$Version = Get-WorkspaceVersion

function Invoke-Native([string]$What, [scriptblock]$Cmd) {
  & $Cmd
  if ($LASTEXITCODE -ne 0) { throw "$What failed (exit code $LASTEXITCODE)" }
}

Push-Location $Root
try {
  if ($Release) {
    Invoke-Native 'cargo build' { cargo build --release -p signet-app -p signet-helper }
  } else {
    Invoke-Native 'cargo build' { cargo build -p signet-app -p signet-helper }
  }
  $Target = Join-Path $Root "target\$Profile_"

  $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
  $CefDir = Get-ChildItem -Path $env:CEF_PATH -Directory -Recurse -Depth 2 -Filter "cef_windows_$arch" -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName 'libcef.dll') } | Select-Object -First 1
  if (-not $CefDir) { throw "CEF (cef_windows_$arch) not found under $env:CEF_PATH; run the cargo build first" }

  # Only the folder we own is removed.
  if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
  New-Item -ItemType Directory -Force -Path $Out | Out-Null

  Copy-Item (Join-Path $Target 'signet.exe') $Out
  Copy-Item (Join-Path $Target 'signet-helper.exe') $Out

  # CEF runtime: everything except the SDK (headers, cmake, wrapper sources), import libraries and debug symbols.
  $skip = @('include', 'cmake', 'libcef_dll', 'CMakeLists.txt', 'archive.json', 'CREDITS.html', 'README.txt')
  foreach ($item in Get-ChildItem -Path $CefDir.FullName) {
    if ($skip -contains $item.Name) { continue }
    if ($item.Extension -in '.lib', '.pdb') { continue }
    Copy-Item $item.FullName -Destination $Out -Recurse -Force
  }

  # Application manifest: per-monitor v2 DPI, Windows 7 to 11, long paths, UTF-8. Embedded when mt.exe exists
  # (the Windows SDK), else shipped as signet.exe.manifest, which Windows reads when the exe has no manifest of its own.
  $manifest = Join-Path $PSScriptRoot 'windows\signet.manifest'
  $mt = Get-Command mt.exe -ErrorAction SilentlyContinue
  if ($mt) {
    foreach ($exe in 'signet.exe', 'signet-helper.exe') {
      & $mt.Source -nologo -manifest $manifest "-outputresource:$(Join-Path $Out $exe);#1"
      if ($LASTEXITCODE -ne 0) {
        Write-Warning "mt.exe failed on $exe (it may already have a manifest); copying the manifest next to it"
        Copy-Item $manifest (Join-Path $Out "$exe.manifest") -Force
      }
    }
  } else {
    Copy-Item $manifest (Join-Path $Out 'signet.exe.manifest') -Force
    Copy-Item $manifest (Join-Path $Out 'signet-helper.exe.manifest') -Force
  }

  # Icon: the PNG as is, and a .ico (one 256 px PNG entry, valid since Windows Vista) made with System.Drawing.
  $png = Join-Path $Root '..\desktop\resources\icons\aurore.png'
  if (Test-Path $png) {
    Copy-Item $png (Join-Path $Out 'signet.png')
    try {
      Add-Type -AssemblyName System.Drawing
      $src = [System.Drawing.Image]::FromFile((Resolve-Path $png).Path)
      $bmp = New-Object System.Drawing.Bitmap 256, 256
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
      $g.DrawImage($src, 0, 0, 256, 256)
      $g.Dispose(); $src.Dispose()
      $ms = New-Object System.IO.MemoryStream
      $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
      $bmp.Dispose()
      $data = $ms.ToArray()
      $ico = New-Object System.IO.MemoryStream
      $w = New-Object System.IO.BinaryWriter $ico
      $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]1)          # ICONDIR: reserved, type icon, 1 image
      $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0); $w.Write([byte]0)  # 256x256 (0), no palette
      $w.Write([uint16]1); $w.Write([uint16]32)                                # planes, bits per pixel
      $w.Write([uint32]$data.Length); $w.Write([uint32]22)                     # size, offset
      $w.Write($data)
      $w.Flush()
      [System.IO.File]::WriteAllBytes((Join-Path $Out 'signet.ico'), $ico.ToArray())
    } catch {
      Write-Warning "could not make signet.ico: $_"
    }
  } else {
    Write-Warning "icon not found: $png"
  }

  Write-Host "Bundled $Out ($Profile_, version $Version). Run: $(Join-Path $Out 'signet.exe')"

  if ($Zip) {
    $archive = Join-Path $Dist "signet-$Version-windows-$arch.zip"
    if (Test-Path $archive) { Remove-Item -Force $archive }
    Compress-Archive -Path $Out -DestinationPath $archive
    Write-Host "Archive $archive"
  }
} finally {
  Pop-Location
}
