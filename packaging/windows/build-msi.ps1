# Build a Windows MSI around the prebuilt nerve.exe.
#
# Requires the WiX Toolset (`dotnet tool install --global wix`) and a
# prebuilt `nerve.exe` under core\target\release.
#
# Usage:
#   .\packaging\windows\build-msi.ps1 -Version 0.1.0

param(
    [string] $Version = "0.1.0"
)

$ErrorActionPreference = "Stop"

$Bin = "core\target\release\nerve.exe"
if (-not (Test-Path $Bin)) {
    Write-Error "Build the daemon first: cargo build --release in core/"
}

$wxs = @"
<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="Nerve" Manufacturer="Nerve Contributors"
           Version="$Version" UpgradeCode="A2C8C9B0-3B26-4D44-9B1F-4D9C7C7C7C7C">
    <MajorUpgrade DowngradeErrorMessage="A newer version of Nerve is already installed." />
    <Feature Id="Main" Title="Nerve daemon" Level="1">
      <Component Id="NerveBin" Directory="INSTALLFOLDER" Guid="*">
        <File Source="$Bin" Name="nerve.exe" />
      </Component>
    </Feature>
    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="Nerve" />
    </StandardDirectory>
  </Package>
</Wix>
"@

$wxsPath = "build\nerve.wxs"
New-Item -ItemType Directory -Force -Path "build" | Out-Null
Set-Content -Path $wxsPath -Value $wxs -Encoding utf8

wix build $wxsPath -o "nerve-$Version.msi"
Write-Output "wrote nerve-$Version.msi"
