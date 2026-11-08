<#
.SYNOPSIS
Pulls the latest source and reinstalls Claude Session Widget.

.DESCRIPTION
Fast-forwards the current branch from its remote, then runs install.ps1 with the same options.
Settings and the autostart choice are preserved.
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'ClaudeSessionWidget\app'),
    [string]$StartMenuDir = [Environment]::GetFolderPath('Programs'),
    [switch]$Autostart,
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'git was not found.' }
if (-not (Test-Path (Join-Path $root '.git'))) { throw "$root is not a git checkout; update it manually, then run install.ps1." }

git -C $root pull --ff-only
if ($LASTEXITCODE -ne 0) { throw 'git pull failed. Resolve it (local changes? diverged branch?) and retry.' }

& (Join-Path $PSScriptRoot 'install.ps1') @PSBoundParameters
