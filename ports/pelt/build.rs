// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // Scripted documents (Boa heap, DOM reflectors, and the Livery session)
        // need more than the Windows process-main stack reserved by the default
        // Rust link. The winit event loop must stay on that main thread.
        println!("cargo:rustc-link-arg-bin=pelt=/STACK:8388608");
    }
}
