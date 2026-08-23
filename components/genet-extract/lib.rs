/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Compatibility shim for the renamed extraction lane.
//!
//! New consumers should depend on [`fleece`] directly. This crate remains for
//! one release so existing downstream manifests continue to compile.

#![deny(unsafe_code)]

pub use fleece::*;
