# Helpers shared by install.ps1, update.ps1 and uninstall.ps1.

$script:RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# Same name as the entry the app's own "Start at login" writes, so both manage one entry.
$script:RunValue = 'Claude Session Widget'

# Only processes started from $installDir are stopped, so another copy (for example a dev build) is left alone.
function Stop-Widget([string]$installDir) {
    $exe = [IO.Path]::GetFullPath((Join-Path $installDir 'ClaudeSessionWidget.exe'))
    $running = Get-Process -Name 'ClaudeSessionWidget' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $exe }
    # Not CloseMainWindow: the widget turns a window close into "hide" and saves that, so the next start would stay hidden.
    foreach ($p in $running) {
        Stop-Process -Id $p.Id -Force
        $p.WaitForExit()
    }
}

function Set-Autostart([string]$exe) {
    Set-ItemProperty -Path $script:RunKey -Name $script:RunValue -Value "`"$exe`""
}

# Removes the autostart entry only if it points into $installDir.
function Remove-Autostart([string]$installDir) {
    $current = (Get-ItemProperty -Path $script:RunKey -Name $script:RunValue -ErrorAction SilentlyContinue).($script:RunValue)
    if ($current -and $current.Contains([IO.Path]::GetFullPath($installDir))) {
        Remove-ItemProperty -Path $script:RunKey -Name $script:RunValue
    }
}
