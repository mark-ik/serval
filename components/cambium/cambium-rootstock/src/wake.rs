// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! A host-neutral callback for waking the native event loop.
//!
//! Workers own their own channels and message vocabularies. They send a value,
//! then call [`HostWake::wake`]; the host schedules one application drain and
//! one redraw. This deliberately has the same callback shape as Armillary's
//! `Wake` alias without making the Cambium host depend on Armillary.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cloneable, thread-safe request to wake one hosted application.
#[derive(Clone)]
pub struct HostWake {
    pending: Arc<AtomicBool>,
    signal: Arc<dyn Fn() + Send + Sync>,
}

impl HostWake {
    pub fn new(pending: Arc<AtomicBool>, signal: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { pending, signal }
    }

    /// Schedule an application drain and redraw, unless one is already queued.
    ///
    /// Coalescing is intentional: a worker may emit several updates before the
    /// event loop gets a turn, and the application drains its own channel in
    /// one wake. A later update that arrives while the drain runs schedules the
    /// next turn normally.
    pub fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.signal)();
        }
    }

    /// The host-neutral callback form used by actor runtimes such as Armillary.
    ///
    /// This returns `Arc<dyn Fn() + Send + Sync>` rather than an Armillary type
    /// so the GUI host remains independent of any particular actor crate.
    pub fn callback(&self) -> Arc<dyn Fn() + Send + Sync> {
        let wake = self.clone();
        Arc::new(move || wake.wake())
    }

    pub(crate) fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

impl fmt::Debug for HostWake {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostWake(..)")
    }
}
