<#
.SYNOPSIS
Run the W3c X11 shadow receipt on a remote screen, with a native probe.

.DESCRIPTION
The X11 half of W3. The app runs as an ordinary X11 client and holds one
headed window open; while it is up, a probe reads that window's own
`_GTK_FRAME_EXTENTS` and its frame geometry off the X server, then releases the
window so the scenario finishes. Two independent witnesses to the same window:
what the app drew, and what the window manager published about it.

**This does not need an X11 login session, and on current Fedora there is not
one to have** — GNOME 50 removed it, so `/usr/share/xsessions` is empty. It
needs an X11 *client*, which XWayland already serves under mutter, and mutter
lists `_GTK_FRAME_EXTENTS` in `_NET_SUPPORTED`.

**So the receipt this writes is an XWayland receipt and says so.** The protocol
exchange is genuine and the window manager implementing it is real, but
mutter-on-Wayland is underneath, so nothing here covers a different window
manager or a non-compositing X server. WSL is not an alternative: WSLg
composites each window into the Windows desktop with no reparenting manager
negotiating extents, so a run there would measure the bridge instead.

.PARAMETER Target
SSH destination of the machine with the graphical session.
#>

[CmdletBinding()]
param(
    [string] $Target = 'markik@192.168.4.28',
    [string] $RemotePath = '/home/markik/Code/repos/genet',
    [ValidateSet('release', 'debug')] [string] $CargoProfile = 'debug',
    [string] $Out
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$codeRoot = Split-Path (Split-Path $repoRoot -Parent) -Parent
if (-not $Out) {
    $stamp = Get-Date -Format 'yyyy-MM-dd_HHmmss'
    $Out = Join-Path $codeRoot "testing\genet\w3c-x11-shadow-$stamp"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path -LiteralPath $Out) {
    throw "output directory already exists: $Out"
}

function Sh($value) { "'" + ($value -replace "'", "'\''") + "'" }

# A fixed path rather than one under the run's own directory: the probe is a
# separate ssh session and has to name the same file the app is watching,
# before the run's timestamped directory exists.
$releaseFile = '/tmp/w3c-x11-release'
$probeOut = '/tmp/w3c-x11-probe.txt'

Write-Host "==> Preparing the X11 probe on $Target" -ForegroundColor Cyan
$remoteStatus = & ssh -o BatchMode=yes $Target "git -C $(Sh $RemotePath) status --porcelain" 2>&1
if ($LASTEXITCODE -ne 0) { throw "cannot inspect $RemotePath on $Target" }
if ($remoteStatus) { throw "remote checkout is dirty:`n$($remoteStatus -join "`n")" }
& ssh -o BatchMode=yes $Target "rm -f $(Sh $releaseFile) $(Sh $probeOut)" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "cannot reach $Target" }

# The probe, as one remote script. It waits for the app's window to exist,
# reads the two properties that matter off that window, and only then releases
# the scenario -- so the measurement is taken while the window is genuinely up
# rather than racing its teardown.
$probeScript = @'
set -u
export DISPLAY=${DISPLAY:-:0}
export XAUTHORITY=$(ls /run/user/$(id -u)/.mutter-Xwaylandauth.* 2>/dev/null | head -1)
out=PROBE_OUT
release=RELEASE_FILE
: > "$out"
echo "display=$DISPLAY" >> "$out"
# The window manager names itself on the window _NET_SUPPORTING_WM_CHECK
# points at, not on the root -- reading the root just reports "not found" and
# leaves the receipt unable to say what it was measured against.
wmcheck=$(xprop -root -notype _NET_SUPPORTING_WM_CHECK 2>/dev/null | sed 's/.*# //' | tr -d ' ')
if [ -n "$wmcheck" ] && [ "$wmcheck" != "0x0" ]; then
  echo "wm=$(xprop -id "$wmcheck" -notype _NET_WM_NAME 2>/dev/null | sed 's/.*= //')" >> "$out"
else
  echo "wm=UNKNOWN" >> "$out"
fi
if xprop -root _NET_SUPPORTED 2>/dev/null | tr ',' '
' | grep -q _GTK_FRAME_EXTENTS; then
  echo "wm_supports_gtk_frame_extents=yes" >> "$out"
else
  echo "wm_supports_gtk_frame_extents=no" >> "$out"
fi
# xprop alone, deliberately: this machine has no xdotool or xwininfo, and a
# receipt is not worth installing packages onto someone's laptop for. The
# client list plus a name check finds the window just as well.
win=""
for _ in $(seq 1 120); do
  for id in $(xprop -root -notype _NET_CLIENT_LIST 2>/dev/null | sed 's/.*# //' | tr ',' ' '); do
    case "$id" in 0x*) ;; *) continue ;; esac
    if xprop -id "$id" -notype _NET_WM_NAME 2>/dev/null | grep -Fq '"host smoke"'; then
      win="$id"
      break
    fi
  done
  [ -n "$win" ] && break
  sleep 1
done
if [ -z "$win" ]; then
  echo "window=NOT_FOUND" >> "$out"
else
  echo "window=$win" >> "$out"
  # The native title can become visible just before the host publishes its
  # client-frame margins. Keep this tied to the same XID while that property
  # settles instead of racing creation or finding a later window.
  gtk_extents=""
  for _ in $(seq 1 50); do
    gtk_extents=$(xprop -id "$win" -notype _GTK_FRAME_EXTENTS 2>/dev/null | sed 's/.*= //')
    case "$gtk_extents" in
      "_GTK_FRAME_EXTENTS:  not found."|"") sleep 0.1 ;;
      *) break ;;
    esac
  done
  echo "gtk_frame_extents=$gtk_extents" >> "$out"
  echo "net_frame_extents=$(xprop -id "$win" -notype _NET_FRAME_EXTENTS 2>/dev/null | sed 's/.*= //')" >> "$out"
fi
# Release only after the reading is on disk, so a failed probe cannot look
# like a passing run that simply measured nothing.
touch "$release"
'@ -replace 'PROBE_OUT', $probeOut -replace 'RELEASE_FILE', $releaseFile

$probeEncoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($probeScript))

Write-Host "==> Starting the headed X11 run" -ForegroundColor Cyan
$remoteReceipt = Join-Path $PSScriptRoot 'remote-receipt.ps1'
$runArgs = @{
    Target = $Target
    Repo = 'genet'
    RemotePath = $RemotePath
    Package = 'cambium-genet-winit-host'
    Example = 'smoke'
    Scenario = 'components/cambium/cambium-genet-winit-host/examples/smoke_x11_shadow.scn'
    ScenarioEnv = 'HOST_SMOKE_SCENARIO'
    CaptureEnv = 'HOST_SMOKE_CAPTURE_DIR'
    ReceiptEnv = 'HOST_SMOKE_RECEIPT'
    Platform = 'linux'
    CargoProfile = $CargoProfile
    Out = $Out
    ExtraEnv = @{
        # Pinned winit chooses Wayland whenever either endpoint is present.
        # Emptying both selectors makes this same binary choose DISPLAY/X11.
        WAYLAND_DISPLAY = ''
        WAYLAND_SOCKET = ''
        HOST_SMOKE_WINDOW_FRAME = 'app'
        HOST_SMOKE_APP_FRAME_INSET = '16'
        HOST_SMOKE_RELEASE_FILE = $releaseFile
        CAMBIUM_HOST_FRAME_TRACE = '1'
    }
}

$run = Start-Job -ScriptBlock {
    param($script, $arguments)
    & $script @arguments 2>&1
} -ArgumentList $remoteReceipt, $runArgs

Write-Host "==> Probing the live window from a second session" -ForegroundColor Cyan
& ssh -o BatchMode=yes $Target "echo $probeEncoded | base64 -d > /tmp/w3c-probe.sh && bash /tmp/w3c-probe.sh" 2>&1 | Out-Null

$runOutput = Receive-Job -Job $run -Wait -AutoRemoveJob
$runOutput | ForEach-Object { Write-Host $_ }

# Bring the probe's reading home beside the app's own captures, so the receipt
# directory holds both witnesses rather than one.
& scp -q "${Target}:$probeOut" (Join-Path $Out 'x11-probe.txt')
if ($LASTEXITCODE -ne 0) { throw 'could not fetch the probe reading' }

$probe = Get-Content (Join-Path $Out 'x11-probe.txt') -Raw
Write-Host "==> Probe reading" -ForegroundColor Cyan
Write-Host $probe

$receipt = Get-Content (Join-Path $Out 'receipt.txt') -Raw -ErrorAction SilentlyContinue
if (-not $receipt) { throw 'the run wrote no receipt' }
if ($receipt -notmatch 'RESULT ok') { throw "the scenario failed:`n$receipt" }
if ($receipt -notmatch 'alpha=0\.\.255 transparent=[1-9][0-9]* translucent=[1-9][0-9]* outer-translucent=[1-9][0-9]*') {
    throw 'the app capture did not prove clear margins plus an outer translucent shadow'
}
if ($probe -match 'window=NOT_FOUND') { throw 'the probe never found the window; nothing was measured' }
if ($probe -match 'wm=UNKNOWN' -or $probe -match 'wm=$') {
    throw 'the probe could not name the window manager; the receipt would not say what it measured against'
}
if ($probe -notmatch 'wm_supports_gtk_frame_extents=yes') {
    throw 'the window manager does not advertise _GTK_FRAME_EXTENTS; this machine cannot witness W3c'
}
if ($probe -notmatch '(?m)^gtk_frame_extents=16, 16, 16, 16$') {
    throw "the client did not publish the expected 16px _GTK_FRAME_EXTENTS:`n$probe"
}
$runLog = Get-Content (Join-Path $Out 'run.log') -Raw -ErrorAction SilentlyContinue
$trace = '[cambium-winit] window-frame backend=x11 policy=App decorated=false transparent=true'
if (-not $runLog.Contains($trace)) {
    throw "the run did not report the expected effective X11 frame: $trace"
}

# remote-receipt writes the common provenance first. Complete it with this
# wrapper's native witness and the logs created after its initial file scan.
$manifestPath = Join-Path $Out 'manifest.json'
$manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
$manifest.platform = 'linux-x11-xwayland'
$manifest | Add-Member -NotePropertyName x11 -NotePropertyValue ([pscustomobject]@{
    window = ([regex]::Match($probe, '(?m)^window=(.+)$').Groups[1].Value)
    gtk_frame_extents = '16, 16, 16, 16'
    compositor = ([regex]::Match($probe, '(?m)^wm=(.+)$').Groups[1].Value)
    trace = $trace
}) -Force
$manifest.artifacts = @(Get-ChildItem -File $Out | Where-Object Name -ne 'manifest.json' | ForEach-Object {
    [pscustomobject]@{
        name = $_.Name
        bytes = $_.Length
        sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    }
})
$manifest | ConvertTo-Json -Depth 8 | Set-Content $manifestPath

Write-Host "==> W3c XWayland receipt in $Out" -ForegroundColor Green
