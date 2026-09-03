// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! `ortet` — open one document in one window. See the crate docs in `lib.rs`.
//!
//! At O0 the binary is the front end only: it parses the command line, resolves
//! the address, and reports which fetch lane will serve it. O1 hands the
//! resolved configuration to the winit shell.

use ortet::args::{self, Invocation};
use ortet::fetch::lane_for;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ortet: {error}");
            std::process::ExitCode::FAILURE
        },
    }
}

fn run() -> Result<(), String> {
    let config = match args::parse(std::env::args().skip(1))? {
        Invocation::Help => {
            print!("{}", args::USAGE);
            return Ok(());
        },
        Invocation::Run(config) => *config,
    };

    println!("ortet: address {}", config.address);
    println!("ortet: lane {:?}", lane_for(&config.address));
    println!(
        "ortet: window {}x{}, frames {:?}, artifact {:?}",
        config.size.0, config.size.1, config.frames, config.artifact
    );
    Err("the ortet viewer lands in O1 of the founding plan".to_owned())
}
