# Copyright 2026 Mark Alan Boykin
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
# SPDX-License-Identifier: MPL-2.0

param(
    [switch]$SkipCargoCheck
)

$ErrorActionPreference = "Stop"

if (-not $SkipCargoCheck) {
    cargo check -p genet-static-html
}

$tree = cargo tree -p genet-static-html
$blocked = $tree | Select-String -Pattern "servo-script|servo-script-bindings|mozjs|servo-media|servo-storage"

if ($blocked) {
    Write-Error ("genet-static-html pulled blocked dependencies:`n" + ($blocked -join "`n"))
}

Write-Host "genet-static-html dependency gate passed"
