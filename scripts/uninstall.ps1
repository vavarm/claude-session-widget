<#
.SYNOPSIS
Removes Claude Session Widget for the current user.

.DESCRIPTION
Stops the widget, removes the autostart entry, the Start Menu shortcut and the installed files.
Settings are kept unless -Purge is given. Nothing is deleted if InstallDir does not look like an installation.

.PARAMETER Purge
Also delete the settings in %APPDATA%\ClaudeSessionWidget.
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'ClaudeSessionWidget\app'),
    [string]$StartMenuDir = [Environment]::GetFolderPath('Programs'),
    [switch]$Purge
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$exe = Join-Path $InstallDir 'ClaudeSessionWidget.exe'
if (-not (Test-Path $exe)) {
    throw "No installation found in $InstallDir (ClaudeSessionWidget.exe is missing); nothing was removed."
}

Stop-Widget $InstallDir
& $exe --remove-hooks | Out-Null
Remove-Autostart $InstallDir

$link = Join-Path $StartMenuDir 'Claude Session Widget.lnk'
if (Test-Path $link) { Remove-Item $link -Force }

Remove-Item $InstallDir -Recurse -Force
$parent = Split-Path $InstallDir -Parent
if ((Test-Path $parent) -and -not (Get-ChildItem $parent -Force)) { Remove-Item $parent -Force }

if ($Purge) {
    $data = if ($env:CLAUDE_WIDGET_DATA_DIR) { $env:CLAUDE_WIDGET_DATA_DIR } else { Join-Path $env:APPDATA 'ClaudeSessionWidget' }
    if (Test-Path $data) { Remove-Item $data -Recurse -Force }
    Write-Host 'Settings removed.'
}
else {
    Write-Host 'Settings kept (use -Purge to remove them).'
}

Write-Host 'Claude Session Widget uninstalled.'
