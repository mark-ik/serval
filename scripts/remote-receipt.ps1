<#
.SYNOPSIS
Run a self-driving scenario on another machine and bring its receipt home.

.DESCRIPTION
The cross-platform half of the headed-verify harness. Every app in the family
already drives itself from a `.scn` file and writes a receipt plus in-process
frame captures; what was missing was a way to do that on the Linux and macOS
boxes and collect the evidence. This is that.

SSH drives; it does not render. A GUI started from an ordinary SSH shell has
no display, and forwarding one would be worse than useless for the things
these receipts exist to check — window decorations, compositor behaviour,
shadows — because `ssh -X` renders against the *local* window manager and
would cheerfully report the wrong answer. So the app runs on the remote
machine's own screen, inside the graphical session already logged in there,
and this script only starts it and fetches what it wrote.

Authentication is whatever SSH is already configured with. If `personae-agent`
is running it serves the vault's SSH slots over the OpenSSH named pipe, so no
key material sits on disk in the clear; the script reports whether it found an
agent but does not require one.

.PARAMETER Target
SSH destination, `user@host`.

.PARAMETER Repo
Repository name. Names the local artifact directory under `Code/testing/`.

.PARAMETER RemotePath
Absolute path to the checkout on the remote machine.

.PARAMETER Package
Cargo package to run (`-p`).

.PARAMETER Example
Cargo example name, if the scenario lane is an example rather than a binary.

.PARAMETER Scenario
Path to the `.scn` file, relative to RemotePath.

.PARAMETER ScenarioEnv
Environment variable the app reads the scenario path from
(`WOODSHED_SCENARIO`, `HOST_SMOKE_SCENARIO`, …).

.PARAMETER CaptureEnv
Environment variable naming the capture output directory. Optional.

.PARAMETER ReceiptEnv
Environment variable naming the receipt file. Optional.

.PARAMETER ExtraEnv
Extra environment variables, e.g. `@{ WOODSHED_STATE = '/tmp/ws-scenario.json' }`.
Named `ExtraEnv` rather than `Env` because `$Env` is PowerShell's environment
provider variable and a parameter of that name shadows it.
Woodshed's scenario lane REQUIRES `WOODSHED_STATE` so a run cannot clobber the
real session.

.PARAMETER Platform
`linux` or `macos`. Decides how the command is attached to the graphical
session.

.PARAMETER CargoProfile
`release` (default) or `debug`. Recorded in the manifest, because a receipt
that does not say which profile it ran under invites the reader to assume the
shipping one. Use `debug` when only the debug target is warm and the receipt
is about what was drawn rather than how fast. Named `CargoProfile` rather
than `Profile` for the same reason `ExtraEnv` is not `Env`: `$PROFILE` is a
PowerShell automatic variable, and a parameter of that name shadows it.

.PARAMETER IngestBin
Path to mere's `receipt_ingest` binary. When given, the fetched receipt is
ingested into the personal graph's blob store in the same motion that fetched
it, so a run on another machine becomes a replicated fact rather than a folder
on this one. Build it with:

    cargo build -p graphshell --features personal-sync --bin receipt_ingest

Genet takes a path rather than knowing where mere lives, because it does not
depend on mere and should not learn its layout.

.PARAMETER IngestStore
The redb database the blobs go into (mere's personal-graph blob store).
Required when `-IngestBin` is given.

.PARAMETER IngestDevice
Name recorded as the device holding the bytes. Defaults to this machine's
hostname; blob availability is per device, so a wrong name claims bytes are
somewhere they are not.

.PARAMETER IngestDataRoot
The resident host's data root. Without it, ingest stores the blobs but leaves
the authored events sitting beside the receipt, so the run never becomes a
fact in the personal graph and the card never appears. Pass it to complete the
hand-off: the resident host picks the events up within ~10s and authors them,
because it, not this script, holds the signing identity and the log.

.EXAMPLE
./remote-receipt.ps1 -Target mark@thinkpad -Repo woodshed `
  -RemotePath /home/mark/Code/repos/woodshed -Package woodshed-genet `
  -Scenario design_docs/scenarios/frame.scn -ScenarioEnv WOODSHED_SCENARIO `
  -CaptureEnv WOODSHED_CAPTURE_DIR -Platform linux `
  -ExtraEnv @{ WOODSHED_STATE = '/tmp/ws-scenario.json' }
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Target,
    [Parameter(Mandatory)] [string] $Repo,
    [Parameter(Mandatory)] [string] $RemotePath,
    [Parameter(Mandatory)] [string] $Package,
    [string] $Example,
    [Parameter(Mandatory)] [string] $Scenario,
    [Parameter(Mandatory)] [string] $ScenarioEnv,
    [string] $CaptureEnv,
    [string] $ReceiptEnv,
    [hashtable] $ExtraEnv = @{},
    [Parameter(Mandatory)] [ValidateSet('linux', 'macos')] [string] $Platform,
    [ValidateSet('release', 'debug')] [string] $CargoProfile = 'release',
    [string] $Out,
    [string] $IngestBin,
    [string] $IngestStore,
    [string] $IngestDevice = $env:COMPUTERNAME,
    [string] $IngestDataRoot
)

$ErrorActionPreference = 'Stop'

function Step($message) { Write-Host "==> $message" -ForegroundColor Cyan }
function Note($message) { Write-Host "    $message" -ForegroundColor DarkGray }
function Warn($message) { Write-Host "    $message" -ForegroundColor Yellow }
function Fail($message) { Write-Host "!!! $message" -ForegroundColor Red; exit 1 }

# Quote for the remote POSIX shell. Everything crossing the wire goes through
# this: a capture directory under a path with a space is not an exotic case.
function Sh($value) { "'" + ($value -replace "'", "'\''") + "'" }

function Invoke-Remote($command) {
    # BatchMode so an unreachable or unauthenticated host fails instead of
    # sitting on a password prompt no one is watching; the build-and-run call
    # can take minutes, so only the connect is bounded.
    $output = & ssh -o BatchMode=yes -o ConnectTimeout=10 $Target $command 2>&1
    [pscustomobject]@{ Output = ($output -join "`n").Trim(); Code = $LASTEXITCODE }
}

$stamp = Get-Date -Format 'yyyy-MM-dd_HHmmss'
$hostLabel = ($Target -split '@')[-1]
if (-not $Out) {
    $Out = Join-Path $PSScriptRoot "../../../testing/$Repo/$hostLabel-$stamp"
}

# ---------------------------------------------------------------- preflight
#
# Each of these is an invariant the run silently depends on. Asserting them up
# front is the difference between a receipt that failed and a receipt that
# lies: an app launched with no display does not error, it simply never draws.

Step "Preflight: $Target ($Platform)"

$agent = if ($IsWindows -or $null -eq $IsWindows) {
    Test-Path '\\.\pipe\openssh-ssh-agent'
} else {
    [bool]$env:SSH_AUTH_SOCK
}
if ($agent) { Note 'ssh agent: present (personae-agent serves vault SSH slots here)' }
else { Warn 'ssh agent: none found; ssh will fall back to on-disk keys' }

$probe = Invoke-Remote 'uname -sr && id -u'
if ($probe.Code -ne 0) { Fail "cannot reach $Target : $($probe.Output)" }
$probeLines = $probe.Output -split "`n"
$remoteOs = $probeLines[0].Trim()
$remoteUid = $probeLines[-1].Trim()
Note "remote: $remoteOs (uid $remoteUid)"

$head = Invoke-Remote "cd $(Sh $RemotePath) && git rev-parse HEAD && git status --porcelain | wc -l"
if ($head.Code -ne 0) { Fail "no checkout at $RemotePath : $($head.Output)" }
$headLines = $head.Output -split "`n"
$remoteCommit = $headLines[0].Trim()
$remoteDirty = [int]($headLines[-1].Trim())
Note "commit: $remoteCommit$(if ($remoteDirty -gt 0) { " (+$remoteDirty dirty files)" })"
if ($remoteDirty -gt 0) {
    Warn 'the remote checkout is dirty; the receipt is not attributable to a commit alone'
}

# The graphical-session check. This is the one that matters most: without it a
# run "succeeds" having drawn nothing anywhere.
if ($Platform -eq 'linux') {
    $session = Invoke-Remote @'
for v in WAYLAND_DISPLAY DISPLAY XDG_RUNTIME_DIR XDG_SESSION_TYPE; do
  printf '%s=%s\n' "$v" "$(systemctl --user show-environment 2>/dev/null | sed -n "s/^$v=//p")"
done
'@
    $sessionVars = @{}
    foreach ($line in ($session.Output -split "`n")) {
        if ($line -match '^([A-Z_]+)=(.*)$') { $sessionVars[$Matches[1]] = $Matches[2] }
    }
    if (-not ($sessionVars['WAYLAND_DISPLAY'] -or $sessionVars['DISPLAY'])) {
        Fail @"
no graphical session found for $Target.
The user must be logged in at the machine's own screen, and the systemd user
manager must know about it. Log in there, then retry. (This is checked rather
than assumed because a headed run with no display draws nothing and still
exits 0 — the receipt would claim a frame that never existed.)
"@
    }
    $sessionKind = if ($sessionVars['WAYLAND_DISPLAY']) { "wayland ($($sessionVars['WAYLAND_DISPLAY']))" }
                   else { "x11 ($($sessionVars['DISPLAY']))" }
    Note "session: $sessionKind"
} else {
    $console = Invoke-Remote 'stat -f%Su /dev/console'
    $sshUser = ($Target -split '@')[0]
    if ($console.Output.Trim() -ne $sshUser) {
        Fail @"
the console user on $hostLabel is '$($console.Output.Trim())', not '$sshUser'.
A GUI can only be launched into the Aqua session of the logged-in console
user. Log in as $sshUser at the machine, then retry.
"@
    }
    Note "session: aqua (console user $sshUser)"
}

# ------------------------------------------------------------------ compose
#
# Artifacts go to a per-run directory on the remote so a rerun never mixes
# with the last one, and so the fetch can take the whole directory blind.

$remoteOut = "/tmp/receipt-$Repo-$stamp"
$remoteReceipt = "$remoteOut/receipt.txt"

$envPairs = [ordered]@{}
$envPairs[$ScenarioEnv] = "$RemotePath/$Scenario"
if ($CaptureEnv) { $envPairs[$CaptureEnv] = $remoteOut }
if ($ReceiptEnv) { $envPairs[$ReceiptEnv] = $remoteReceipt }
foreach ($key in $ExtraEnv.Keys) { $envPairs[$key] = $ExtraEnv[$key] }

$envAssignments = ($envPairs.GetEnumerator() | ForEach-Object {
    "$($_.Key)=$(Sh $_.Value)"
}) -join ' '

$cargoArgs = if ($Example) { "-p $Package --example $Example" } else { "-p $Package" }
$profileFlag = if ($CargoProfile -eq 'release') { '--release ' } else { '' }
$runLine = "cargo run $profileFlag$cargoArgs"

# `systemd-run --user` puts the command in the user manager's own environment,
# which is where the graphical session's variables live. macOS needs no
# equivalent: `launchctl asuser` is root-only, and an ssh session belonging to
# the console user can open a window on that user's screen directly. What
# makes that safe is the preflight, which refuses to run when the ssh user is
# not the console user.
# Built in two steps on purpose: `+` binds tighter than `-join`, so folding
# these into one expression concatenates an array onto a string and then
# "joins" the result with itself. Each `--setenv=` token is quoted whole,
# because a capture directory under a path with a space is ordinary.
$setenvTokens = ($envPairs.GetEnumerator() | ForEach-Object {
    Sh "--setenv=$($_.Key)=$($_.Value)"
}) -join ' '
# `--working-directory` is not optional: a transient unit does NOT inherit the
# ssh shell's cwd, so without it cargo runs in $HOME and fails to find a
# Cargo.toml. The `cd` below still matters for the macOS branch, which runs in
# the shell rather than in a unit.
$attached = if ($Platform -eq 'linux') {
    "systemd-run --user --wait --collect --pipe --working-directory=$(Sh $RemotePath) " +
    "--setenv=RUST_BACKTRACE=1 $setenvTokens"
} else { $null }

# A non-interactive ssh shell does not source the login profile, so cargo is
# not on PATH even where it is plainly installed. Prepending is enough and is
# harmless where the profile would have done it anyway.
$cargoPath = 'export PATH="$HOME/.cargo/bin:$PATH"; '

$remoteCommand = if ($Platform -eq 'linux') {
    "mkdir -p $(Sh $remoteOut) && cd $(Sh $RemotePath) && $cargoPath$attached -- $runLine"
} else {
    # No `launchctl asuser`: it requires root, and over ssh as an ordinary user
    # it fails with "Could not switch to audit session". It is also not needed
    # here — the preflight above has already asserted that the ssh user IS the
    # console user, and a GUI launched by that user from ssh appears on their
    # own screen. The app's own receipt is the backstop: a run that reached no
    # display captures blank, identical frames, which its frame checks fail.
    "mkdir -p $(Sh $remoteOut) && cd $(Sh $RemotePath) && " +
    "$cargoPath env $envAssignments RUST_BACKTRACE=1 $runLine"
}

Step 'Building and running the scenario on the remote screen'
Note "scenario: $Scenario"
Note "artifacts: $remoteOut"
$run = Invoke-Remote $remoteCommand
$runCode = $run.Code
if ($runCode -eq 0) { Note 'run: exit 0' } else { Warn "run: exit $runCode" }

# ------------------------------------------------------------------- fetch

Step 'Fetching artifacts'
$listing = Invoke-Remote "ls -1 $(Sh $remoteOut) 2>/dev/null | wc -l"
$artifactCount = [int]($listing.Output.Trim())
if ($artifactCount -eq 0) {
    Fail @"
the run produced no artifacts in $remoteOut (exit $runCode).
Nothing is being copied back, because an empty receipt directory is a failed
run, not a passing one. The run's output was:

$($run.Output)
"@
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null
# The whole directory, not `dir/*`: current OpenSSH scp speaks SFTP and no
# longer expands remote globs, so a wildcard silently fetches nothing.
& scp -q -r "${Target}:$remoteOut" $Out
if ($LASTEXITCODE -ne 0) { Fail 'scp failed' }
# scp -r lands the directory itself; lift its contents up so the artifact
# directory is flat and the manifest sits beside what it describes.
$landed = Join-Path $Out (Split-Path $remoteOut -Leaf)
if (Test-Path $landed) {
    Get-ChildItem -Force $landed | Move-Item -Destination $Out -Force
    Remove-Item $landed -Recurse -Force
}
$fetched = Get-ChildItem -Recurse -File $Out
Note "fetched $($fetched.Count) file(s) to $Out"

# ---------------------------------------------------------------- manifest
#
# The receipt about the receipt: what ran, where, against which commit. A
# capture with no provenance cannot be trusted six months later, and these
# are exactly the artifacts that get looked at six months later.

$manifest = [ordered]@{
    repo         = $Repo
    package      = $Package
    example      = $Example
    scenario     = $Scenario
    target       = $Target
    platform     = $Platform
    profile      = $CargoProfile
    remote_os    = $remoteOs
    remote_path  = $RemotePath
    remote_commit = $remoteCommit
    remote_dirty = $remoteDirty
    session      = if ($Platform -eq 'linux') { $sessionKind } else { 'aqua' }
    ran_at_utc   = (Get-Date).ToUniversalTime().ToString('o')
    exit_code    = $runCode
    env          = $envPairs
    artifacts    = @($fetched | ForEach-Object {
        [ordered]@{
            name   = $_.Name
            bytes  = $_.Length
            sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
        }
    })
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Out 'manifest.json')

# The run's own stdout, kept whole. The app's receipt says what it asserted;
# this says what it printed getting there.
$run.Output | Set-Content (Join-Path $Out 'run.log')

Step 'Summary'
Note "$Repo on $hostLabel ($remoteOs)"
Note "commit $($remoteCommit.Substring(0, 12))$(if ($remoteDirty -gt 0) { ' + local edits' })"
foreach ($file in $fetched | Sort-Object Name) { Note "  $($file.Name) ($($file.Length) bytes)" }

$appReceipt = Get-ChildItem $Out -Filter '*.receipt' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($appReceipt) {
    Write-Host ''
    Get-Content $appReceipt.FullName | ForEach-Object { Write-Host "    $_" }
}

# ------------------------------------------------------------------ ingest
#
# The receipt becomes a replicated graph fact. Deliberately after the manifest
# is written, so what gets ingested is what was recorded, and deliberately not
# fatal: a fetched receipt is worth keeping even if the graph is unavailable.

if ($IngestBin) {
    Step 'Ingesting into the personal graph'
    if (-not $IngestStore) { Fail '-IngestBin needs -IngestStore' }
    if (-not (Test-Path $IngestBin)) {
        Fail "no receipt_ingest at $IngestBin (cargo build -p graphshell --features personal-sync --bin receipt_ingest)"
    }
    $ingestArgs = @('--dir', $Out, '--store', $IngestStore, '--device', $IngestDevice)
    if ($IngestDataRoot) { $ingestArgs += @('--data-root', $IngestDataRoot) }
    $ingest = & $IngestBin @ingestArgs 2>&1
    if ($LASTEXITCODE -eq 0) {
        $ingest | ForEach-Object { Note $_ }
    } else {
        Warn "ingest failed (exit $LASTEXITCODE); the receipt is still in $Out"
        $ingest | ForEach-Object { Warn $_ }
    }
}

Write-Host ''
if ($runCode -ne 0) {
    Warn "the run exited $runCode — artifacts were kept, but this is not a pass"
    exit $runCode
}
Write-Host "Receipt in $Out" -ForegroundColor Green
if (-not $IngestBin) {
    Write-Host "Pass -IngestBin/-IngestStore to file it in the personal graph." -ForegroundColor DarkGray
}
