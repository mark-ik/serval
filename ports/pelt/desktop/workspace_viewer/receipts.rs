// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Named workspace receipt drivers.
//!
//! Each receipt is a scripted, headed run that asserts one product claim.
//! `routing` owns the step machine the others advance through.

mod a11y;
mod chrome;
mod reader;
mod routing;
