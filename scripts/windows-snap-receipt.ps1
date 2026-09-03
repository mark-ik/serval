# Copyright 2026 Mark Alan Boykin
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
# SPDX-License-Identifier: MPL-2.0

<#
.SYNOPSIS
Prove Windows Snap Layout hover for the real Cambium app-frame maximize control.

.DESCRIPTION
Runs the headed host smoke, reads its laid-out maximize rectangle, asks the
same HWND for WM_NCHITTEST at that rectangle, moves the real pointer there,
and captures the visible desktop before releasing the app-authored scenario.
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
    $Out = Join-Path $codeRoot "testing\genet\w4-windows-snap-$stamp"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path -LiteralPath $Out) {
    throw "output directory already exists: $Out"
}
[void](New-Item -ItemType Directory -Path $Out)

$scenario = Join-Path $repoRoot 'components\cambium\cambium-genet-winit-host\examples\smoke_windows_snap.scn'
$appReceipt = Join-Path $Out 'receipt.txt'
$nativeReceipt = Join-Path $Out 'native-snap.txt'
$releaseFile = Join-Path $Out 'native-inspection-complete'
$screenshot = Join-Path $Out 'snap-layout-hover.png'
$stdout = Join-Path $Out 'run.stdout.log'
$stderr = Join-Path $Out 'run.stderr.log'

if (-not ('W4Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class W4Native {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetClientRect(IntPtr hwnd, out RECT rect);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageW(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool BringWindowToTop(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetWindowPos(
        IntPtr hwnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, IntPtr processId);

    [DllImport("user32.dll")]
    public static extern bool AttachThreadInput(uint attach, uint attachTo, bool value);

    [DllImport("user32.dll")]
    public static extern int GetSystemMetrics(int index);

    [DllImport("user32.dll")]
    public static extern bool SetProcessDPIAware();

    [DllImport("kernel32.dll")]
    public static extern uint GetCurrentThreadId();
}
'@
}
[void][W4Native]::SetProcessDPIAware()

function ConvertTo-HitTestLParam([int] $X, [int] $Y) {
    $packed = (([int64] $Y -band 0xffffL) -shl 16) -bor ([int64] $X -band 0xffffL)
    [IntPtr] $packed
}

function Save-VirtualScreen([string] $Path) {
    Add-Type -AssemblyName System.Drawing
    $left = [W4Native]::GetSystemMetrics(76)
    $top = [W4Native]::GetSystemMetrics(77)
    $width = [W4Native]::GetSystemMetrics(78)
    $height = [W4Native]::GetSystemMetrics(79)
    if ($width -le 0 -or $height -le 0) {
        throw "invalid virtual screen ${width}x${height}"
    }
    $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CopyFromScreen($left, $top, 0, 0, $bitmap.Size, [Drawing.CopyPixelOperation]::SourceCopy)
        } finally {
            $graphics.Dispose()
        }
        $bitmap.Save($Path, [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $bitmap.Dispose()
    }
}

$commit = (& git -C $repoRoot rev-parse HEAD).Trim()
$dirtyFiles = @(& git -C $repoRoot status --porcelain=v1)
$launchedAt = Get-Date
$cargoProcess = $null
$windowProcess = $null
$nativeResult = 'missing'

$envNames = @(
    'HOST_SMOKE_WINDOW_FRAME',
    'HOST_SMOKE_SCENARIO',
    'HOST_SMOKE_RECEIPT',
    'HOST_SMOKE_CAPTURE_DIR',
    'HOST_SMOKE_RELEASE_FILE',
    'CAMBIUM_HOST_FRAME_TRACE',
    'RUST_BACKTRACE'
)
$savedEnv = @{}
foreach ($name in $envNames) {
    $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

try {
    $env:HOST_SMOKE_WINDOW_FRAME = 'app'
    $env:HOST_SMOKE_SCENARIO = $scenario
    $env:HOST_SMOKE_RECEIPT = $appReceipt
    $env:HOST_SMOKE_CAPTURE_DIR = $Out
    $env:HOST_SMOKE_RELEASE_FILE = $releaseFile
    $env:CAMBIUM_HOST_FRAME_TRACE = '1'
    $env:RUST_BACKTRACE = '1'

    $cargo = (Get-Command cargo -ErrorAction Stop).Source
    $cargoProcess = Start-Process -FilePath $cargo `
        -ArgumentList @('run', '--offline', '-p', 'cambium-genet-winit-host', '--example', 'smoke') `
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

    $traceMatch = $null
    for ($attempt = 0; $attempt -lt 400; $attempt++) {
        if (Test-Path -LiteralPath $stderr) {
            $trace = Get-Content -LiteralPath $stderr -Raw
            $traceMatch = [regex]::Match(
                $trace,
                'snap-layout hit=HTMAXBUTTON rect=\[(-?\d+),(-?\d+),(-?\d+),(-?\d+)\] scale=([^\s]+)'
            )
            if ($traceMatch.Success) { break }
        }
        if ($windowProcess.HasExited) { break }
        Start-Sleep -Milliseconds 50
    }
    if (-not $traceMatch -or -not $traceMatch.Success) {
        throw 'the host never published a maximize hit rectangle'
    }

    $left = [int] $traceMatch.Groups[1].Value
    $top = [int] $traceMatch.Groups[2].Value
    $right = [int] $traceMatch.Groups[3].Value
    $bottom = [int] $traceMatch.Groups[4].Value
    if ($right -le $left -or $bottom -le $top) {
        throw "invalid maximize hit rectangle [$left,$top,$right,$bottom]"
    }

    $windowProcess.Refresh()
    $hwnd = [IntPtr] $windowProcess.MainWindowHandle
    $client = New-Object W4Native+RECT
    if (-not [W4Native]::GetClientRect($hwnd, [ref] $client)) {
        throw "GetClientRect failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    if ($left -lt $client.Left -or $top -lt $client.Top -or $right -gt $client.Right -or $bottom -gt $client.Bottom) {
        throw "maximize rect [$left,$top,$right,$bottom] is outside client [$($client.Left),$($client.Top),$($client.Right),$($client.Bottom)]"
    }

    $clientOrigin = New-Object W4Native+POINT
    if (-not [W4Native]::ClientToScreen($hwnd, [ref] $clientOrigin)) {
        throw "ClientToScreen failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $centerX = $clientOrigin.X + [int] (($left + $right) / 2)
    $centerY = $clientOrigin.Y + [int] (($top + $bottom) / 2)
    $outsideX = $clientOrigin.X + [Math]::Max($client.Left + 2, $left - 8)
    $outsideY = $centerY
    $insideHit = [W4Native]::SendMessageW($hwnd, 0x0084, [IntPtr]::Zero, (ConvertTo-HitTestLParam $centerX $centerY)).ToInt64()
    $outsideHit = [W4Native]::SendMessageW($hwnd, 0x0084, [IntPtr]::Zero, (ConvertTo-HitTestLParam $outsideX $outsideY)).ToInt64()
    if ($insideHit -ne 9) {
        throw "WM_NCHITTEST at maximize center returned $insideHit instead of HTMAXBUTTON (9)"
    }
    if ($outsideHit -eq 9) {
        throw 'WM_NCHITTEST leaked HTMAXBUTTON outside the maximize rectangle'
    }

    $positionFlags = 0x0001 -bor 0x0002 -bor 0x0040
    $foregroundBefore = [W4Native]::GetForegroundWindow()
    $currentThread = [W4Native]::GetCurrentThreadId()
    $foregroundThread = [W4Native]::GetWindowThreadProcessId($foregroundBefore, [IntPtr]::Zero)
    $targetThread = [W4Native]::GetWindowThreadProcessId($hwnd, [IntPtr]::Zero)
    $attachedForeground = $false
    $attachedTarget = $false
    try {
        if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
            $attachedForeground = [W4Native]::AttachThreadInput($currentThread, $foregroundThread, $true)
        }
        if ($targetThread -ne 0 -and $targetThread -ne $currentThread -and $targetThread -ne $foregroundThread) {
            $attachedTarget = [W4Native]::AttachThreadInput($currentThread, $targetThread, $true)
        }
        if (-not [W4Native]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, $positionFlags)) {
            throw "SetWindowPos(HWND_TOPMOST) failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        [void][W4Native]::BringWindowToTop($hwnd)
        [void][W4Native]::SetForegroundWindow($hwnd)
        Start-Sleep -Milliseconds 350
        if ([W4Native]::GetForegroundWindow() -ne $hwnd) {
            throw "foreground HWND is 0x$([W4Native]::GetForegroundWindow().ToInt64().ToString('x')), expected 0x$($hwnd.ToInt64().ToString('x'))"
        }
    } finally {
        if ($attachedTarget) {
            [void][W4Native]::AttachThreadInput($currentThread, $targetThread, $false)
        }
        if ($attachedForeground) {
            [void][W4Native]::AttachThreadInput($currentThread, $foregroundThread, $false)
        }
    }
    [void][W4Native]::SetCursorPos($outsideX, $outsideY)
    Start-Sleep -Milliseconds 250
    if (-not [W4Native]::SetCursorPos($centerX, $centerY)) {
        throw 'SetCursorPos failed for the maximize center'
    }
    Start-Sleep -Milliseconds 1600
    Save-VirtualScreen $screenshot
    if (-not [W4Native]::SetWindowPos($hwnd, [IntPtr](-2), 0, 0, 0, 0, $positionFlags)) {
        throw "SetWindowPos(HWND_NOTOPMOST) failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }

    $nativeResult = 'ok'
    @(
        'RESULT ok'
        "process $($windowProcess.Id) hwnd=0x$($hwnd.ToInt64().ToString('x'))"
        "client [$($client.Left),$($client.Top),$($client.Right),$($client.Bottom)]"
        "maximize-device-rect [$left,$top,$right,$bottom] scale=$($traceMatch.Groups[5].Value)"
        "maximize-screen-center [$centerX,$centerY]"
        "WM_NCHITTEST center=$insideHit outside=$outsideHit"
        "foreground hwnd=0x$($hwnd.ToInt64().ToString('x'))"
        'headed-capture snap-layout-hover.png'
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
    scenario = 'components/cambium/cambium-genet-winit-host/examples/smoke_windows_snap.scn'
    platform = 'windows'
    commit = $commit
    dirty_files = $dirtyFiles
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
