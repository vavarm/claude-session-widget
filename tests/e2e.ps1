# End-to-end test of the real executable against a fake sessions directory.
#
# It never touches the mouse, the keyboard focus or the real settings:
#   - the page is inspected through the WebView2 DevTools protocol (private debug port),
#   - menu entries are run through the app's `menu_action` command,
#   - settings, sessions and the WebView2 profile live in a throwaway directory.
# The widget window does appear (without taking focus) while the test runs.
#
# Windows only. Usage: powershell -NoProfile -File tests\e2e.ps1
# (run `cargo build --release --manifest-path src-tauri\Cargo.toml` first)
$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot '..\src-tauri\target\release\ClaudeSessionWidget.exe' | Resolve-Path

Add-Type @"
using System; using System.Runtime.InteropServices;
public class Win {
  public delegate bool CB(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
}
"@

$failures = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "PASS  $name" }
    else { Write-Host "FAIL  $name $detail"; $script:failures++ }
}

function Wait-Until([scriptblock]$condition, [int]$timeoutSec = 8) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        $result = & $condition
        if ($result) { return $result }
        Start-Sleep -Milliseconds 100
    }
    $false
}

function Find-Window($procId) {
    $global:foundHwnd = [IntPtr]::Zero
    $global:foundArea = 0
    $global:targetPid = $procId
    # The process also owns small helper windows (tray, WebView2); the widget is the biggest visible one.
    [Win]::EnumWindows({ param($h, $l)
        $p = 0; [void][Win]::GetWindowThreadProcessId($h, [ref]$p)
        if ($p -eq $global:targetPid -and [Win]::IsWindowVisible($h)) {
            $r = New-Object Win+RECT; [void][Win]::GetWindowRect($h, [ref]$r)
            $area = ($r.R - $r.L) * ($r.B - $r.T)
            if ($area -gt $global:foundArea) { $global:foundArea = $area; $global:foundHwnd = $h }
        }
        $true }, [IntPtr]::Zero) | Out-Null
    if ($global:foundArea -gt 10000) { $global:foundHwnd }
}

function Get-Rect($hwnd) { $r = New-Object Win+RECT; [void][Win]::GetWindowRect($hwnd, [ref]$r); $r }
function Win-Height($hwnd) { $r = Get-Rect $hwnd; $r.B - $r.T }
function Expected-Height($hwnd, $rows, $compact = $false) {
    $row = if ($compact) { 28 } else { 46 }
    (34 + [Math]::Max($rows, 1) * $row + 8) * [Win]::GetDpiForWindow($hwnd) / 96.0
}
function Height-Is($hwnd, $rows, $compact = $false) { [Math]::Abs((Win-Height $hwnd) - (Expected-Height $hwnd $rows $compact)) -le 2 }

# --- DevTools protocol client -------------------------------------------------------------
$script:cdpId = 0
function Cdp-Connect($port) {
    $page = Wait-Until { try { Invoke-RestMethod "http://127.0.0.1:$port/json" | Where-Object { $_.type -eq 'page' } | Select-Object -First 1 } catch { $false } } 15
    if (-not $page) { throw 'The WebView2 debug endpoint did not come up.' }
    $script:ws = New-Object Net.WebSockets.ClientWebSocket
    [void]$script:ws.ConnectAsync([uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
}

function Cdp-Eval($expression) {
    $script:cdpId++
    $msg = @{ id = $script:cdpId; method = 'Runtime.evaluate'; params = @{ expression = $expression; returnByValue = $true } } | ConvertTo-Json -Compress -Depth 5
    $cts = New-Object Threading.CancellationTokenSource 5000
    [void]$script:ws.SendAsync([ArraySegment[byte]]::new([Text.Encoding]::UTF8.GetBytes($msg)), 'Text', $true, $cts.Token).GetAwaiter().GetResult()
    while ($true) {
        $buffer = New-Object byte[] 65536
        $stream = New-Object IO.MemoryStream
        do {
            $r = $script:ws.ReceiveAsync([ArraySegment[byte]]::new($buffer), $cts.Token).GetAwaiter().GetResult()
            $stream.Write($buffer, 0, $r.Count)
        } until ($r.EndOfMessage)
        $reply = [Text.Encoding]::UTF8.GetString($stream.ToArray()) | ConvertFrom-Json
        if ($reply.id -eq $script:cdpId) { return $reply.result.result.value }
    }
}

function Page-State {
    Cdp-Eval "JSON.stringify({ rows: [...document.querySelectorAll('.row')].map(r => r.classList[1]), empty: !!document.querySelector('.empty'), compact: document.getElementById('card').classList.contains('compact') })" | ConvertFrom-Json
}

# Runs a menu entry by id through the app's own handler (the native popup needs a mouse).
function Click-Menu([string]$id) {
    [void](Cdp-Eval "void window.__TAURI__.core.invoke('menu_action', { id: '$id' })")
    $true
}

# --- Fixtures -----------------------------------------------------------------------------
function New-Dummy { Start-Process powershell -ArgumentList '-NoProfile', '-Command', 'Start-Sleep 600' -WindowStyle Hidden -PassThru }

function Write-Session($dir, $pidValue, $startFileTime, $status, $name) {
    $json = @{ pid = $pidValue; procStart = [string]$startFileTime; cwd = "C:\work\$name"
               name = $name; status = $status; statusUpdatedAt = [DateTimeOffset]::Now.ToUnixTimeMilliseconds() } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText((Join-Path $dir "$pidValue.json"), $json, (New-Object Text.UTF8Encoding $false))
}

function Read-Settings { Get-Content (Join-Path $appdata 'settings.json') -Raw | ConvertFrom-Json }

# The WebView2 child processes can outlive the app briefly and keep the debug port; wait for it to close
# so the next instance's page is not mistaken for the old one.
function Stop-App {
    Stop-Process $script:app -Force
    [void](Wait-Until {
        $client = New-Object Net.Sockets.TcpClient
        try { $client.Connect('127.0.0.1', $port); $false } catch { $true } finally { $client.Dispose() }
    } 15)
}

function Start-App {
    $script:app = Start-Process $exe -PassThru
    $script:hwnd = Wait-Until { Find-Window $script:app.Id } 15
    if ($script:hwnd) {
        Cdp-Connect $port
        # Commands only work once the page has rendered its first state.
        [void](Wait-Until { Cdp-Eval "!!window.__TAURI__ && !!document.querySelector('.head')" } 15)
    }
}

$work = Join-Path ([IO.Path]::GetTempPath()) "sw-e2e-$([guid]::NewGuid().ToString('N'))"
$sessions = Join-Path $work 'sessions'; $appdata = Join-Path $work 'appdata'
New-Item -ItemType Directory $sessions, $appdata | Out-Null
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0); $listener.Start()
$port = $listener.LocalEndpoint.Port; $listener.Stop()
$dummies = @(); $app = $null; $hwnd = $null

try {
    $a = New-Dummy; $b = New-Dummy; $c = New-Dummy; $dummies = @($a, $b, $c)
    foreach ($d in $dummies) { [void](Wait-Until { $d.StartTime } 5) }
    Write-Session $sessions $a.Id $a.StartTime.ToFileTime() 'waiting_for_input' 'alpha'
    Write-Session $sessions $b.Id $b.StartTime.ToFileTime() 'busy' 'bravo'
    Write-Session $sessions $c.Id $c.StartTime.ToFileTime() 'idle' 'charlie'
    Write-Session $sessions 2147483000 1 'busy' 'ghost'

    $env:CLAUDE_WIDGET_SESSIONS_DIR = $sessions
    $env:CLAUDE_WIDGET_DATA_DIR = $appdata
    $env:WEBVIEW2_USER_DATA_FOLDER = Join-Path $work 'webview'
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$port"

    Start-App
    Check 'widget window is visible' ($null -ne $hwnd -and $hwnd)
    Check 'window is topmost' (Wait-Until { ([Win]::GetWindowLong($hwnd, -20) -band 8) -ne 0 } 3) ('exstyle=0x{0:X}' -f [Win]::GetWindowLong($hwnd, -20))

    $state = Wait-Until { $s = Page-State; if ($s.rows.Count -eq 3) { $s } } 10
    Check 'ghost (dead pid) ignored; rows sorted needs-you, busy, idle' ($state -and ($state.rows -join ',') -eq 'attention,busy,idle') ($state | ConvertTo-Json -Compress)
    Check 'window fits 3 rows' (Wait-Until { Height-Is $hwnd 3 }) "got $(Win-Height $hwnd)"

    Check 'menu: compact mode' ((Click-Menu 'compact') -and (Wait-Until { (Page-State).compact }) -and (Wait-Until { Height-Is $hwnd 3 $true }) -and (Read-Settings).Compact)
    Check 'menu: compact toggles back' ((Click-Menu 'compact') -and (Wait-Until { -not (Page-State).compact }) -and (Wait-Until { Height-Is $hwnd 3 }) -and -not (Read-Settings).Compact)
    Check 'menu: opacity 60 %' ((Click-Menu 'opacity_60') -and (Wait-Until { [Math]::Abs((Read-Settings).Opacity - 0.6) -lt 0.01 }) -and (Wait-Until { (Cdp-Eval "document.getElementById('card').style.getPropertyValue('--opacity')") -eq '0.6' }))
    Check 'menu: always-on-top off' ((Click-Menu 'top') -and (Wait-Until { ([Win]::GetWindowLong($hwnd, -20) -band 8) -eq 0 }) -and -not (Read-Settings).AlwaysOnTop)

    Stop-Process $b -Force
    Check 'killed process removes its row' ((Wait-Until { (Page-State).rows.Count -eq 2 } 10) -and (Wait-Until { Height-Is $hwnd 2 })) "got $(Win-Height $hwnd)"

    Write-Session $sessions $a.Id $a.StartTime.ToFileTime() 'idle' 'alpha'
    Check 'status change is picked up' (Wait-Until { (Page-State).rows[0] -eq 'idle' } 10)

    Stop-Process $a, $c -Force
    Check 'no live session: empty state' ((Wait-Until { (Page-State).empty } 10) -and (Wait-Until { Height-Is $hwnd 0 }))

    [void][Win]::SetWindowPos($hwnd, [IntPtr]::Zero, 321, 123, 0, 0, 0x0001 -bor 0x0004 -bor 0x0010)
    $saved = Wait-Until { $s = Read-Settings; if ($s.X -eq 321 -and $s.Y -eq 123) { $s } } 5
    Check 'moved position is persisted' ([bool]$saved) "got $((Read-Settings).X),$((Read-Settings).Y)"

    Stop-App
    Start-App
    $r = Get-Rect $hwnd
    Check 'position restored after restart' ($r.L -eq 321 -and $r.T -eq 123) "got $($r.L),$($r.T)"

    [void](Click-Menu 'show')
    $hidden = Wait-Until { -not [Win]::IsWindowVisible($hwnd) }
    $savedHidden = Wait-Until { -not (Read-Settings).WidgetVisible }
    Check 'menu: hide widget' ($hidden -and $savedHidden) "hidden=$hidden saved=$savedHidden settings=$(Get-Content (Join-Path $appdata 'settings.json') -Raw)"
    Stop-App
    $app = Start-Process $exe -PassThru
    Start-Sleep -Seconds 2
    Check 'hidden state restored after restart' (-not (Find-Window $app.Id))
    Check 'widget process is still running while hidden' (-not $app.HasExited)
}
finally {
    if ($app) { Stop-Process $app -Force -ErrorAction SilentlyContinue }
    $dummies | ForEach-Object { Stop-Process $_ -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures -gt 0) { Write-Host "$failures check(s) failed"; exit 1 }
Write-Host 'All e2e checks passed'
