// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Scrolltext parser — re-exported from
//! [`scroll-protocol`](https://crates.io/crates/scroll-protocol).
//!
//! The grammar lives with its protocol under the smolweb home decision; the
//! path here keeps errand's parse family in one place. Richer than gemtext by
//! design: five heading levels, nested quotes and lists with verbatim ordered
//! markers, tagged code blocks, input links, link relations, inline markup
//! ([`spans`]), and linetype escaping.

pub use scroll_protocol::scrolltext::{
    Polarity, Relation, ScrollLine, Span, SpanKind, parse, spans,
};
