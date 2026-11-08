<#
.SYNOPSIS
Builds Claude Session Widget from source and installs it for the current user (Windows).

.DESCRIPTION
Safe to run repeatedly: it is also the update step. The new build is staged in a temporary
directory first, and the previous installation is only replaced once the build succeeded
(and restored if the copy fails). Settings are never touched.

.PARAMETER InstallDir
Where the app is installed. Defaults to %LOCALAPPDATA%\ClaudeSessionWidget\app.

.PARAMETER StartMenuDir
Directory that receives the Start Menu shortcut. Defaults to the current user's Programs folder.

.PARAMETER Autostart
Start the widget when Windows starts. Without this switch an existing autostart entry is left as is.

.PARAMETER Context
Show each session's context use, model and cost. This registers the widget as Claude Code's status line
in ~/.claude/settings.json (an existing status line is never replaced). Can also be toggled from the tray menu.

.PARAMETER Agents
Show running agents and background tasks under each session. This registers the widget as Claude Code's
subagent status line in ~/.claude/settings.json (an existing one is never replaced). Can also be toggled from the tray menu.

.PARAMETER NoLaunch
Do not start the widget after installing.
#>
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'ClaudeSessionWidget\app'),
    [string]$StartMenuDir = [Environment]::GetFolderPath('Programs'),
    [switch]$Autostart,
    [switch]$Context,
    [switch]$Agents,
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')

$root = Split-Path $PSScriptRoot -Parent
$exeName = 'ClaudeSessionWidget.exe'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'The Rust toolchain was not found. Install it from https://rustup.rs and retry.'
}

$stage = Join-Path ([IO.Path]::GetTempPath()) "claude-session-widget-$([guid]::NewGuid().ToString('N'))"
$backup = "$InstallDir.old"

try {
    Write-Host 'Building (the first build downloads and compiles dependencies and can take several minutes)...'
    cargo build --release --manifest-path (Join-Path $root 'src-tauri\Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE." }

    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'src-tauri\target' }
    $built = Join-Path $targetDir "release\$exeName"
    if (-not (Test-Path $built)) { throw "Build output does not contain $exeName." }
    New-Item -ItemType Directory $stage | Out-Null
    Copy-Item $built $stage

    Stop-Widget $InstallDir

    if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }
    if (Test-Path $InstallDir) { Move-Item $InstallDir $backup }
    try {
        New-Item -ItemType Directory (Split-Path $InstallDir -Parent) -Force | Out-Null
        Copy-Item $stage $InstallDir -Recurse
    }
    catch {
        if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force }
        if (Test-Path $backup) { Move-Item $backup $InstallDir }
        throw
    }
    if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }
}
finally {
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
}

$exe = Join-Path $InstallDir $exeName

New-Item -ItemType Directory $StartMenuDir -Force | Out-Null
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut((Join-Path $StartMenuDir 'Claude Session Widget.lnk'))
$link.TargetPath = $exe
$link.WorkingDirectory = $InstallDir
$link.Description = 'Always-on-top widget that tracks your Claude Code sessions'
$link.Save()

if ($Autostart) { Set-Autostart $exe }

foreach ($option in @(@($Context, 'context'), @($Agents, 'agents'))) {
    if ($option[0]) {
        # Piping the output makes PowerShell wait for this GUI-subsystem executable to finish.
        $message = & $exe "--enable-$($option[1])" | Out-String
        if ($LASTEXITCODE -ne 0) { Write-Warning "Option '$($option[1])' was not enabled: $($message.Trim())" }
    }
}

$version = (Get-Item $exe).VersionInfo.ProductVersion
Write-Host "Installed Claude Session Widget $version to $InstallDir"

# `start` hands the process over to the shell so the widget is not tied to this one.
if (-not $NoLaunch) { cmd /c start '""' "`"$exe`"" }
