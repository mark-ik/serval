<#
.SYNOPSIS
Prove that the real Cambium CSD window stays inside the Windows work area when maximized.

.DESCRIPTION
Runs the host smoke's Windows-only maximize scenario, observes that exact HWND
with Win32, and pairs the native geometry result with the app-authored receipt
and in-process frames. The app waits on a fresh release file while maximized,
so native inspection does not race a guessed frame count.
#>

[CmdletBinding()]
param(
    [string] $Out
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$codeRoot = Split-Path (Split-Path $repoRoot -Parent) -Parent
if (-not $Out) {
    $stamp = Get-Date -Format 'yyyy-MM-dd_HHmmss'
    $Out = Join-Path $codeRoot "testing\genet\w3a-windows-maximized-$stamp"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path -LiteralPath $Out) {
    throw "output directory already exists: $Out"
}
[void](New-Item -ItemType Directory -Path $Out)

$scenario = Join-Path $repoRoot 'components\cambium\cambium-genet-winit-host\examples\smoke_windows_maximized.scn'
$appReceipt = Join-Path $Out 'receipt.txt'
$nativeReceipt = Join-Path $Out 'native-geometry.txt'
$releaseFile = Join-Path $Out 'native-inspection-complete'
$stdout = Join-Path $Out 'run.stdout.log'
$stderr = Join-Path $Out 'run.stderr.log'

if (-not ('W3aNative' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class W3aNative {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO {
        public uint Size;
        public RECT Monitor;
        public RECT Work;
        public uint Flags;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetClientRect(IntPtr hwnd, out RECT rect);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);

    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool GetMonitorInfo(IntPtr monitor, ref MONITORINFO info);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsZoomed(IntPtr hwnd);
}
'@
}

function Get-NativeGeometry([IntPtr] $Hwnd) {
    $outer = New-Object W3aNative+RECT
    $client = New-Object W3aNative+RECT
    if (-not [W3aNative]::GetWindowRect($Hwnd, [ref] $outer)) {
        throw "GetWindowRect failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    if (-not [W3aNative]::GetClientRect($Hwnd, [ref] $client)) {
        throw "GetClientRect failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    $clientTopLeft = New-Object W3aNative+POINT
    $clientTopLeft.X = $client.Left
    $clientTopLeft.Y = $client.Top
    $clientBottomRight = New-Object W3aNative+POINT
    $clientBottomRight.X = $client.Right
    $clientBottomRight.Y = $client.Bottom
    if (-not [W3aNative]::ClientToScreen($Hwnd, [ref] $clientTopLeft) -or
        -not [W3aNative]::ClientToScreen($Hwnd, [ref] $clientBottomRight)) {
        throw "ClientToScreen failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    $monitor = [W3aNative]::MonitorFromWindow($Hwnd, 2)
    if ($monitor -eq [IntPtr]::Zero) { throw 'MonitorFromWindow returned no monitor' }
    $monitorInfo = New-Object W3aNative+MONITORINFO
    $monitorInfo.Size = [Runtime.InteropServices.Marshal]::SizeOf([type]'W3aNative+MONITORINFO')
    if (-not [W3aNative]::GetMonitorInfo($monitor, [ref] $monitorInfo)) {
        throw "GetMonitorInfo failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    [pscustomobject]@{
        Outer = $outer
        Client = [pscustomobject]@{
            Left = $clientTopLeft.X
            Top = $clientTopLeft.Y
            Right = $clientBottomRight.X
            Bottom = $clientBottomRight.Y
        }
        Work = $monitorInfo.Work
        Maximized = [W3aNative]::IsZoomed($Hwnd)
    }
}

function Format-Rect($Rect) {
    "[$($Rect.Left),$($Rect.Top),$($Rect.Right),$($Rect.Bottom)]"
}

$commit = (& git -C $repoRoot rev-parse HEAD).Trim()
$dirty = @(& git -C $repoRoot status --porcelain).Count
$launchedAt = Get-Date
$cargoProcess = $null
$windowProcess = $null
$nativeResult = 'missing'

$envNames = @(
    'HOST_SMOKE_SCENARIO',
    'HOST_SMOKE_RECEIPT',
    'HOST_SMOKE_CAPTURE_DIR',
    'HOST_SMOKE_RELEASE_FILE',
    'RUST_BACKTRACE'
)
$savedEnv = @{}
foreach ($name in $envNames) {
    $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

try {
    $env:HOST_SMOKE_SCENARIO = $scenario
    $env:HOST_SMOKE_RECEIPT = $appReceipt
    $env:HOST_SMOKE_CAPTURE_DIR = $Out
    $env:HOST_SMOKE_RELEASE_FILE = $releaseFile
    $env:RUST_BACKTRACE = '1'

    $cargo = (Get-Command cargo -ErrorAction Stop).Source
    $cargoProcess = Start-Process -FilePath $cargo `
        -ArgumentList @('run', '-p', 'cambium-genet-winit-host', '--example', 'smoke') `
        -WorkingDirectory $repoRoot -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
} finally {
    foreach ($name in $envNames) {
        [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process')
    }
}

try {
    for ($attempt = 0; $attempt -lt 2400; $attempt++) {
        $windowProcess = Get-Process -Name 'smoke' -ErrorAction SilentlyContinue |
            Where-Object {
                $_.MainWindowTitle -eq 'host smoke' -and
                $_.MainWindowHandle -ne [IntPtr]::Zero -and
                $_.StartTime -ge $launchedAt.AddSeconds(-1)
            } |
            Select-Object -First 1
        if ($windowProcess) { break }
        if ($cargoProcess.HasExited) { break }
        Start-Sleep -Milliseconds 250
    }
    if (-not $windowProcess) {
        throw 'the headed host smoke window never appeared'
    }

    $hwnd = [IntPtr] $windowProcess.MainWindowHandle
    for ($attempt = 0; $attempt -lt 400; $attempt++) {
        if ([W3aNative]::IsZoomed($hwnd)) { break }
        if ($windowProcess.HasExited) { break }
        Start-Sleep -Milliseconds 50
        $windowProcess.Refresh()
        $hwnd = [IntPtr] $windowProcess.MainWindowHandle
    }
    if (-not [W3aNative]::IsZoomed($hwnd)) {
        throw 'the scenario never maximized the host smoke window'
    }

    $geometry = Get-NativeGeometry $hwnd
    $left = $geometry.Client.Left - $geometry.Work.Left
    $top = $geometry.Client.Top - $geometry.Work.Top
    $right = $geometry.Work.Right - $geometry.Client.Right
    $bottom = $geometry.Work.Bottom - $geometry.Client.Bottom
    $nativeOk = $geometry.Maximized -and $left -eq 0 -and $top -eq 0 -and $right -eq 0 -and $bottom -eq 0
    $nativeResult = if ($nativeOk) { 'ok' } else { 'fail' }
    @(
        "RESULT $nativeResult"
        "process $($windowProcess.Id) hwnd=0x$($hwnd.ToInt64().ToString('x'))"
        "maximized $($geometry.Maximized.ToString().ToLowerInvariant())"
        "outer $(Format-Rect $geometry.Outer)"
        "client $(Format-Rect $geometry.Client)"
        "work $(Format-Rect $geometry.Work)"
        "client-work-gaps left=$left top=$top right=$right bottom=$bottom"
    ) | Set-Content -LiteralPath $nativeReceipt -Encoding utf8
} finally {
    if (-not (Test-Path -LiteralPath $releaseFile)) {
        [void](New-Item -ItemType File -Path $releaseFile)
    }
}

if (-not $cargoProcess.WaitForExit(120000)) {
    throw 'cargo did not exit after native inspection released the scenario'
}
$cargoProcess.Refresh()

$appResult = if (Test-Path -LiteralPath $appReceipt) {
    $line = Get-Content -LiteralPath $appReceipt | Where-Object { $_ -match '^RESULT\s+' } | Select-Object -First 1
    if ($line -match '^RESULT\s+(\S+)') { $Matches[1] } else { 'missing' }
} else {
    'missing'
}

$artifacts = @(Get-ChildItem -LiteralPath $Out -File | Where-Object Name -ne 'manifest.json' | ForEach-Object {
    [ordered]@{
        name = $_.Name
        bytes = $_.Length
        sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
})
[ordered]@{
    repo = 'genet'
    scenario = 'components/cambium/cambium-genet-winit-host/examples/smoke_windows_maximized.scn'
    platform = 'windows'
    commit = $commit
    dirty = $dirty
    ran_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    cargo_exit_code = $cargoProcess.ExitCode
    app_result = $appResult
    native_result = $nativeResult
    artifacts = $artifacts
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Out 'manifest.json') -Encoding utf8

Get-Content -LiteralPath $nativeReceipt
if (Test-Path -LiteralPath $appReceipt) { Get-Content -LiteralPath $appReceipt }
Write-Host "Receipt in $Out"

if ($cargoProcess.ExitCode -ne 0 -or $appResult -ne 'ok' -or $nativeResult -ne 'ok') {
    exit 1
}
