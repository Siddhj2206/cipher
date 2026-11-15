# Cipher's Windows packages from a release build: the installer (Inno Setup 6, `iscc`) and a
# portable zip.
# Usage: pwsh packaging/windows/package.ps1 [-Exe path\to\cipher.exe] [-Out out-dir]
param(
  [string]$Exe = "",
  [string]$Out = ""
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$root = Resolve-Path "$here\..\.."
if (-not $Exe) { $Exe = "$root\target\release\cipher.exe" }
if (-not $Out) { $Out = "$root\target\dist" }
if (-not (Test-Path $Exe)) { throw "no binary at $Exe (cargo build --release -p cipher-app)" }
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$version = (Select-String -Path "$root\Cargo.toml" -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
$name = "Cipher-$version-windows-x64"

# Portable: just the exe (it keeps its settings in %APPDATA%\cipher like the installed one).
$zipDir = Join-Path $env:TEMP $name
Remove-Item -Recurse -Force $zipDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $zipDir | Out-Null
Copy-Item $Exe "$zipDir\cipher.exe"
Compress-Archive -Path "$zipDir\*" -DestinationPath "$Out\$name-portable.zip" -Force
Write-Host "wrote $Out\$name-portable.zip"

$iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
if (-not (Test-Path $iscc)) { throw "Inno Setup 6 not found (choco install innosetup)" }
& $iscc "/DAppVersion=$version" "/DSourceExe=$(Resolve-Path $Exe)" "/DOutputDir=$Out" "$here\cipher.iss"
if ($LASTEXITCODE -ne 0) { throw "iscc failed" }
Write-Host "wrote $Out\$name-setup.exe"
