# Copyright 2026 Mark Alan Boykin
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
# SPDX-License-Identifier: MPL-2.0

<#
.SYNOPSIS
Run the paired W3b host-frame and app-frame receipts on a remote Wayland screen.

.DESCRIPTION
Uses the existing remote receipt lane twice against one checkout. Each run
pairs app-authored frame captures with winit's post-configure decoration answer,
then this wrapper requires the exact complementary results: Host is decorated,
App is undecorated, and both runs identify the Wayland backend.
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
    $Out = Join-Path $codeRoot "testing\genet\w3b-wayland-frame-$stamp"
}
$Out = [System.IO.Path]::GetFullPath($Out)
if (Test-Path -LiteralPath $Out) {
    throw "output directory already exists: $Out"
}

$remoteReceipt = Join-Path $PSScriptRoot 'remote-receipt.ps1'
$common = @{
    Target = $Target
    Repo = 'genet'
    RemotePath = $RemotePath
    Package = 'cambium-genet-winit-host'
    Example = 'smoke'
    ScenarioEnv = 'HOST_SMOKE_SCENARIO'
    CaptureEnv = 'HOST_SMOKE_CAPTURE_DIR'
    ReceiptEnv = 'HOST_SMOKE_RECEIPT'
    Platform = 'linux'
    CargoProfile = $CargoProfile
}

foreach ($mode in @('host', 'app')) {
    $title = (Get-Culture).TextInfo.ToTitleCase($mode)
    $expected = if ($mode -eq 'host') { 'true' } else { 'false' }
    $scenario = "components/cambium/cambium-genet-winit-host/examples/smoke_wayland_${mode}_frame.scn"
    $modeOut = Join-Path $Out $mode
    & $remoteReceipt @common -Scenario $scenario -Out $modeOut -ExtraEnv @{
        HOST_SMOKE_WINDOW_FRAME = $mode
        CAMBIUM_HOST_FRAME_TRACE = '1'
    }
    if ($LASTEXITCODE -ne 0) {
        throw "$mode Wayland frame run failed with exit $LASTEXITCODE"
    }

    $receipt = Join-Path $modeOut 'receipt.txt'
    $runLog = Join-Path $modeOut 'run.log'
    if (-not (Select-String -LiteralPath $receipt -Pattern '^RESULT ok$' -Quiet)) {
        throw "$mode app receipt did not return RESULT ok"
    }
    $trace = "[cambium-winit] window-frame backend=wayland policy=$title decorated=$expected"
    if (-not (Select-String -LiteralPath $runLog -SimpleMatch $trace -Quiet)) {
        throw "$mode run did not report the expected effective frame: $trace"
    }
}

Write-Host "Paired Wayland frame receipt in $Out" -ForegroundColor Green
