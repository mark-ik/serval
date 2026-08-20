<#
.SYNOPSIS
Run the W3c client-drawn X11 shadow receipt on a remote headed session.

.DESCRIPTION
Forces the Cambium smoke onto X11/Xwayland, holds that exact window open,
captures its native X11 properties and client pixels, then releases the app's
semantic scenario. The proof is deliberately paired: `_GTK_FRAME_EXTENTS`
describes transparent margins; the app-authored frame capture must contain
both clear margin pixels and translucent shadow pixels.
#>

[CmdletBinding()]
param(
    [string] $Target = 'markik@192.168.4.28',
    [string] $RemotePath = '/home/markik/Code/repos/genet',
    [ValidateSet('release', 'debug')] [string] $CargoProfile = 'debug',
    [string] $Out
)

$ErrorActionPreference = 'Stop'

function Step($message) { Write-Host "==> $message" -ForegroundColor Cyan }
function Note($message) { Write-Host "    $message" -ForegroundColor DarkGray }
function Sh($value) { "'" + ($value -replace "'", "'\''") + "'" }

function Invoke-Remote($command) {
    $output = & ssh -o BatchMode=yes -o ConnectTimeout=10 $Target $command 2>&1
    [pscustomobject]@{ Output = ($output -join "`n").Trim(); Code = $LASTEXITCODE }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$codeRoot = Split-Path (Split-Path $repoRoot -Parent) -Parent
$stamp = Get-Date -Format 'yyyy-MM-dd_HHmmss'
if (-not $Out) {
    $Out = Join-Path $codeRoot "testing\genet\w3c-x11-shadow-$stamp"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path -LiteralPath $Out) { throw "output directory already exists: $Out" }

$unit = "genet-w3c-$($stamp -replace '[-_]', '')"
$remoteOut = "/tmp/receipt-genet-w3c-$stamp"
$remoteReceipt = "$remoteOut/receipt.txt"
$releaseFile = "$remoteOut/native-inspection-complete"
$scenario = "$RemotePath/components/cambium/cambium-genet-winit-host/examples/smoke_x11_shadow.scn"

Step "Preflight: $Target"
$preflightCommand = @"
set -eu
cd $(Sh $RemotePath)
git rev-parse HEAD
git status --porcelain | wc -l
systemctl --user show-environment | sed -n 's/^DISPLAY=//p'
pgrep -a Xwayland | sed -n 's/.* -auth \([^ ]*\).*/\1/p' | head -1
command -v xprop
command -v import
command -v identify
"@
$preflight = Invoke-Remote $preflightCommand
if ($preflight.Code -ne 0) { throw "remote preflight failed: $($preflight.Output)" }
$lines = $preflight.Output -split "`n"
if ($lines.Count -lt 7) { throw "remote preflight returned incomplete data: $($preflight.Output)" }
$remoteCommit = $lines[0].Trim()
$remoteDirty = [int]$lines[1].Trim()
$display = $lines[2].Trim()
$xauthority = $lines[3].Trim()
if ($remoteDirty -ne 0) { throw "remote checkout has $remoteDirty dirty file(s)" }
if (-not $display -or -not $xauthority) { throw 'headed X11/Xwayland session is unavailable' }
Note "commit: $($remoteCommit.Substring(0, 12))"
Note "display: $display"

$findWindowScript = @'
for id in $(xprop -root _NET_CLIENT_LIST 2>/dev/null | grep -o '0x[0-9a-fA-F]*'); do
  if xprop -id "$id" _NET_WM_NAME 2>/dev/null | grep -Fq '"host smoke"'; then
    printf '%s\n' "$id"
    exit 0
  fi
done
exit 1
'@
$findWindow = "DISPLAY=$(Sh $display) XAUTHORITY=$(Sh $xauthority) sh -c $(Sh $findWindowScript)"
$existing = Invoke-Remote $findWindow
if ($existing.Code -eq 0) { throw "an existing host smoke X11 window would make identity ambiguous: $($existing.Output)" }

$profileFlag = if ($CargoProfile -eq 'release') { '--release ' } else { '' }
$launch = "test ! -e $(Sh $remoteOut) && mkdir -p $(Sh $remoteOut) && " +
    "systemd-run --user --unit=$(Sh $unit) --collect --working-directory=$(Sh $RemotePath) " +
    "--setenv=$(Sh 'PATH=/home/markik/.cargo/bin:/usr/local/bin:/usr/bin') " +
    "--setenv=$(Sh "DISPLAY=$display") --setenv=$(Sh "XAUTHORITY=$xauthority") " +
    # Pinned winit selects Wayland whenever its endpoint is present. Emptying
    # both Wayland selectors makes the same binary choose DISPLAY/X11.
    "--setenv=$(Sh 'WAYLAND_DISPLAY=') --setenv=$(Sh 'WAYLAND_SOCKET=') " +
    "--setenv=$(Sh 'HOST_SMOKE_WINDOW_FRAME=app') " +
    "--setenv=$(Sh 'HOST_SMOKE_APP_FRAME_INSET=16') " +
    "--setenv=$(Sh 'CAMBIUM_HOST_FRAME_TRACE=1') " +
    "--setenv=$(Sh "HOST_SMOKE_SCENARIO=$scenario") " +
    "--setenv=$(Sh "HOST_SMOKE_RECEIPT=$remoteReceipt") " +
    "--setenv=$(Sh "HOST_SMOKE_CAPTURE_DIR=$remoteOut") " +
    "--setenv=$(Sh "HOST_SMOKE_RELEASE_FILE=$releaseFile") " +
    "/home/markik/.cargo/bin/cargo run $profileFlag-p cambium-genet-winit-host --example smoke"

Step 'Launching the forced-X11 app frame'
$started = Invoke-Remote $launch
if ($started.Code -ne 0) { throw "could not start transient unit: $($started.Output)" }

$xid = $null
$deadline = [DateTime]::UtcNow.AddMinutes(5)
while ([DateTime]::UtcNow -lt $deadline) {
    $found = Invoke-Remote $findWindow
    if ($found.Code -eq 0 -and $found.Output -match '^0x[0-9a-fA-F]+$') {
        $xid = $found.Output.Trim()
        break
    }
    $state = Invoke-Remote "systemctl --user is-active $(Sh "$unit.service") 2>/dev/null || true"
    if ($state.Output -notin @('active', 'activating')) {
        $log = Invoke-Remote "journalctl --user -u $(Sh "$unit.service") --no-pager -n 80"
        throw "app exited before native inspection: $($log.Output)"
    }
    Start-Sleep -Seconds 1
}
if (-not $xid) { throw 'timed out waiting for the host smoke X11 window' }
Note "window: $xid"

Step 'Capturing native property geometry and X11 client pixels'
$inspect = @"
set -eu
export DISPLAY=$(Sh $display)
export XAUTHORITY=$(Sh $xauthority)
xprop -id $(Sh $xid) _NET_WM_NAME _NET_WM_PID _NET_WM_WINDOW_TYPE _NET_FRAME_EXTENTS _GTK_FRAME_EXTENTS > $(Sh "$remoteOut/x11-window.txt")
xprop -root _NET_SUPPORTED > $(Sh "$remoteOut/x11-root-supported.txt")
import -window $(Sh $xid) $(Sh "$remoteOut/native-client.png")
identify -format 'size=%wx%h channels=%[channels] alpha-min=%[fx:minima.a] alpha-max=%[fx:maxima.a]\n' $(Sh "$remoteOut/native-client.png") > $(Sh "$remoteOut/native-client-image.txt")
touch $(Sh $releaseFile)
"@
$observed = Invoke-Remote $inspect
if ($observed.Code -ne 0) { throw "native inspection failed: $($observed.Output)" }

$deadline = [DateTime]::UtcNow.AddMinutes(2)
while ([DateTime]::UtcNow -lt $deadline) {
    $done = Invoke-Remote "test -f $(Sh $remoteReceipt) && grep -q '^RESULT ' $(Sh $remoteReceipt)"
    if ($done.Code -eq 0) { break }
    Start-Sleep -Seconds 1
}
$done = Invoke-Remote "test -f $(Sh $remoteReceipt) && grep -q '^RESULT ' $(Sh $remoteReceipt)"
if ($done.Code -ne 0) { throw 'app did not finish after native inspection released it' }

$journal = Invoke-Remote "journalctl --user -u $(Sh "$unit.service") --no-pager > $(Sh "$remoteOut/run.log")"
if ($journal.Code -ne 0) { throw "could not retain unit journal: $($journal.Output)" }

Step 'Fetching and validating the receipt'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
& scp -q -r "${Target}:$remoteOut" $Out
if ($LASTEXITCODE -ne 0) { throw 'scp failed' }
$landed = Join-Path $Out (Split-Path $remoteOut -Leaf)
Get-ChildItem -Force $landed | Move-Item -Destination $Out -Force
Remove-Item -LiteralPath $landed -Recurse -Force

$receiptPath = Join-Path $Out 'receipt.txt'
$nativePath = Join-Path $Out 'x11-window.txt'
$runLog = Join-Path $Out 'run.log'
if (-not (Select-String -LiteralPath $receiptPath -Pattern '^RESULT ok$' -Quiet)) {
    throw 'app-authored receipt did not return RESULT ok'
}
if (-not (Select-String -LiteralPath $receiptPath -Pattern 'alpha=0\.\.255 transparent=[1-9][0-9]* translucent=[1-9][0-9]*' -Quiet)) {
    throw 'app captures did not prove clear margins plus translucent shadow pixels'
}
if (-not (Select-String -LiteralPath $nativePath -Pattern '^_GTK_FRAME_EXTENTS\(CARDINAL\) = 16, 16, 16, 16$' -Quiet)) {
    throw 'native window did not publish the expected _GTK_FRAME_EXTENTS'
}
$trace = '[cambium-winit] window-frame backend=x11 policy=App decorated=false transparent=true'
if (-not (Select-String -LiteralPath $runLog -SimpleMatch $trace -Quiet)) {
    throw "run did not report the expected effective X11 frame: $trace"
}

$files = Get-ChildItem -File $Out
$manifest = [ordered]@{
    repo = 'genet'
    package = 'cambium-genet-winit-host'
    example = 'smoke'
    scenario = 'components/cambium/cambium-genet-winit-host/examples/smoke_x11_shadow.scn'
    target = $Target
    platform = 'linux-x11-xwayland'
    profile = $CargoProfile
    remote_path = $RemotePath
    remote_commit = $remoteCommit
    remote_dirty = $remoteDirty
    display = $display
    x11_window = $xid
    ran_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    env = [ordered]@{
        WAYLAND_DISPLAY = ''
        WAYLAND_SOCKET = ''
        HOST_SMOKE_WINDOW_FRAME = 'app'
        HOST_SMOKE_APP_FRAME_INSET = '16'
        CAMBIUM_HOST_FRAME_TRACE = '1'
    }
    artifacts = @($files | ForEach-Object {
        [ordered]@{
            name = $_.Name
            bytes = $_.Length
            sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
        }
    })
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Out 'manifest.json')

Write-Host ''
Get-Content $receiptPath | ForEach-Object { Write-Host "    $_" }
Write-Host ''
Write-Host "X11 shadow receipt in $Out" -ForegroundColor Green
