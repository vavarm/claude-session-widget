# Tests for scripts\install.ps1, update.ps1 and uninstall.ps1 using throwaway directories.
# The registry and the real installation are never touched (no -Autostart, custom InstallDir).
# Usage: powershell -NoProfile -File tests\scripts.tests.ps1
$ErrorActionPreference = 'Stop'
$scripts = Join-Path $PSScriptRoot '..\scripts' | Resolve-Path

$failures = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "PASS  $name" }
    else { Write-Host "FAIL  $name $detail"; $script:failures++ }
}

# Runs a script in a child PowerShell so `throw` and `exit` cannot abort this test.
function Run($script, [string[]]$arguments, $scriptsDir = $scripts) {
    $ErrorActionPreference = 'Continue'
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $scriptsDir $script) @arguments 2>&1
    [pscustomobject]@{ Code = $LASTEXITCODE; Output = ($out | Out-String) }
}

$work = Join-Path ([IO.Path]::GetTempPath()) "sw-scripts-$([guid]::NewGuid().ToString('N'))"
$app = Join-Path $work 'inst\app'; $menu = Join-Path $work 'menu'; $data = Join-Path $work 'data'
New-Item -ItemType Directory $work, $data | Out-Null
$common = @('-InstallDir', $app, '-StartMenuDir', $menu, '-NoLaunch')
$env:CLAUDE_WIDGET_DATA_DIR = $data
$claudeConfig = Join-Path $work 'claude'
$env:CLAUDE_CONFIG_DIR = $claudeConfig

try {
    $r = Run 'install.ps1' $common
    Check 'install succeeds' ($r.Code -eq 0) $r.Output
    Check 'install puts the exe in InstallDir' (Test-Path (Join-Path $app 'ClaudeSessionWidget.exe'))
    Check 'install creates the Start Menu shortcut' (Test-Path (Join-Path $menu 'Claude Session Widget.lnk'))
    Check 'install leaves no backup or staging dir' (-not (Test-Path "$app.old"))

    $stale = Join-Path $app 'stale.txt'; Set-Content $stale 'x'
    Set-Content (Join-Path $data 'settings.json') '{}'
    $running = Start-Process (Join-Path $app 'ClaudeSessionWidget.exe') -PassThru
    Start-Sleep 2
    $r = Run 'install.ps1' $common
    Check 're-install (update path) succeeds' ($r.Code -eq 0) $r.Output
    Check 're-install stops the running instance' ($running.HasExited)
    Check 're-install replaces old files' (-not (Test-Path $stale))
    Check 're-install keeps settings' (Test-Path (Join-Path $data 'settings.json'))
    Check 're-install does not leave the widget hidden for the next start' ((Get-Content (Join-Path $data 'settings.json') -Raw) -notmatch '"WidgetVisible"\s*:\s*false')

    $r = Run 'install.ps1' ($common + '-Context', '-Agents')
    $claudeSettings = Join-Path $claudeConfig 'settings.json'
    Check 'install -Context registers the status line' ($r.Code -eq 0 -and (Get-Content $claudeSettings -Raw) -match 'ClaudeSessionWidget\.exe.*--statusline') $r.Output
    Check 'install -Agents registers the subagent status line' ((Get-Content $claudeSettings -Raw) -match 'subagentStatusLine')
    Check 'install -Context turns the setting on' ((Get-Content (Join-Path $data 'settings.json') -Raw) -match '"ShowContext"\s*:\s*true')

    $bogus = Join-Path $work 'not-an-install'; New-Item -ItemType Directory $bogus | Out-Null
    Set-Content (Join-Path $bogus 'precious.txt') 'x'
    $r = Run 'uninstall.ps1' @('-InstallDir', $bogus, '-StartMenuDir', $menu)
    Check 'uninstall refuses a directory that is not an install' ($r.Code -ne 0 -and (Test-Path (Join-Path $bogus 'precious.txt')))

    $r = Run 'uninstall.ps1' @('-InstallDir', $app, '-StartMenuDir', $menu)
    Check 'uninstall succeeds' ($r.Code -eq 0) $r.Output
    Check 'uninstall removes the files' (-not (Test-Path $app))
    Check 'uninstall removes both status lines' ((Get-Content $claudeSettings -Raw) -notmatch '(?i)statusLine')
    Check 'uninstall removes the shortcut' (-not (Test-Path (Join-Path $menu 'Claude Session Widget.lnk')))
    Check 'uninstall keeps settings by default' (Test-Path (Join-Path $data 'settings.json'))

    Run 'install.ps1' $common | Out-Null
    $r = Run 'uninstall.ps1' @('-InstallDir', $app, '-StartMenuDir', $menu, '-Purge')
    Check 'uninstall -Purge removes settings' ($r.Code -eq 0 -and -not (Test-Path $data)) $r.Output

    # update.ps1: an origin repo gets a new version after the checkout was cloned from it.
    $repo = Join-Path $PSScriptRoot '..' | Resolve-Path
    $origin = Join-Path $work 'origin'; $clone = Join-Path $work 'clone'
    New-Item -ItemType Directory $origin | Out-Null
    foreach ($dir in 'src', 'scripts') { Copy-Item (Join-Path $repo $dir) $origin -Recurse }
    robocopy (Join-Path $repo 'src-tauri') (Join-Path $origin 'src-tauri') /E /XD target gen | Out-Null
    # Reuse the dependency build cache: the clone only differs by its version number.
    $env:CARGO_TARGET_DIR = Join-Path $repo 'src-tauri\target'
    $git = @('-c', 'user.name=test', '-c', 'user.email=test@example.com')
    git -C $origin init -q -b main
    git -C $origin @git add -A
    git -C $origin @git commit -q -m initial
    git clone -q $origin $clone

    $cargo = Join-Path $origin 'src-tauri\Cargo.toml'
    $text = [IO.File]::ReadAllText($cargo)
    [IO.File]::WriteAllText($cargo, ([regex]'(?m)^version = "[^"]*"').Replace($text, 'version = "9.9.9"', 1))
    git -C $origin @git commit -q -am 'bump version'

    $result = Run 'update.ps1' $common (Join-Path $clone 'scripts')
    $r = $result.Output
    $exe = Join-Path $app 'ClaudeSessionWidget.exe'
    Check 'update pulls and installs the new version' ((Test-Path $exe) -and (Get-Item $exe).VersionInfo.ProductVersion -like '9.9.9*') $r
}
finally {
    Get-Process -Name ClaudeSessionWidget -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($work) } | Stop-Process -Force
    # The clone's build lands in the shared target dir and would otherwise be taken for an up-to-date build of this checkout.
    if ($env:CARGO_TARGET_DIR) {
        $ErrorActionPreference = 'Continue'
        cargo clean --release -p claude-session-widget --manifest-path (Join-Path $PSScriptRoot '..\src-tauri\Cargo.toml') 2>&1 | Out-Null
    }
    Remove-Item Env:CLAUDE_WIDGET_DATA_DIR, Env:CLAUDE_CONFIG_DIR, Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures -gt 0) { Write-Host "$failures check(s) failed"; exit 1 }
Write-Host 'All script checks passed'
