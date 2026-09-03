// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! DOM events in the host's vocabulary.
//!
//! The conversion the winit source does for keyboards, wheels and composition,
//! done once more for the browser. It is the whole of what an event source is:
//! nothing here decides anything, it only says what happened in words the host
//! already knows.

use cambium::CompositionEvent;
use cambium_rootstock::{Key, KeyPress, Modifiers, NamedKey};
use web_sys::{KeyboardEvent, WheelEvent};

/// A DOM `key` value in the host's named-key vocabulary.
///
/// The DOM names keys as strings from the UI Events spec. Keys outside the set
/// the host routes become [`NamedKey::Other`], the same way winit's do.
fn named_from_dom(key: &str) -> Option<NamedKey> {
    Some(match key {
        "Backspace" => NamedKey::Backspace,
        "Enter" => NamedKey::Enter,
        "Tab" => NamedKey::Tab,
        "Escape" => NamedKey::Escape,
        " " | "Spacebar" => NamedKey::Space,
        "ArrowLeft" | "Left" => NamedKey::ArrowLeft,
        "ArrowRight" | "Right" => NamedKey::ArrowRight,
        "ArrowUp" | "Up" => NamedKey::ArrowUp,
        "ArrowDown" | "Down" => NamedKey::ArrowDown,
        "Delete" | "Del" => NamedKey::Delete,
        "Home" => NamedKey::Home,
        "End" => NamedKey::End,
        "PageUp" => NamedKey::PageUp,
        "PageDown" => NamedKey::PageDown,
        _ => return None,
    })
}

/// Modifiers held during a DOM event.
///
/// `metaKey` is the platform command key, Command on macOS and the Windows or
/// Super key elsewhere, which is what the host means by `meta`.
pub fn modifiers_from_dom(event: &KeyboardEvent) -> Modifiers {
    Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

/// A DOM `keydown` as the host routes it.
///
/// The DOM reports one string for both cases the host separates. A `key` of
/// exactly one grapheme is text; anything longer is a name. `"Dead"` is a dead
/// key, which the host keeps distinct so an accent awaiting composition cannot
/// fall into the injected-text path and start typing. `"Unidentified"` is the
/// browser's own name for a key it could not resolve, which is the same case
/// Windows reports to winit as `VK_PACKET`, so assistive input reaches the
/// application through the path already built for it.
pub fn key_press_from_dom(event: &KeyboardEvent) -> KeyPress {
    let dom_key = event.key();
    let key = match dom_key.as_str() {
        "Dead" => Key::Dead,
        "Unidentified" => Key::Unidentified,
        other => match named_from_dom(other) {
            Some(named) => Key::Named(named),
            // One grapheme is text; a longer name this vocabulary does not
            // special-case is a named key it simply does not route.
            None if other.chars().count() == 1 => Key::Character(other.to_string()),
            None => Key::Named(NamedKey::Other),
        },
    };
    let text = match &key {
        Key::Character(c) => Some(c.clone()),
        // An unidentified key may still carry what it produced.
        Key::Unidentified => Some(dom_key.clone()).filter(|k| k != "Unidentified"),
        _ => None,
    };
    KeyPress {
        key,
        text,
        modifiers: modifiers_from_dom(event),
        repeat: event.repeat(),
    }
}

/// How far a wheel notch scrolls, in logical pixels.
///
/// `deltaMode` says what the numbers mean: pixels, lines, or pages. The host
/// takes logical pixels, so lines and pages are resolved here, the same
/// normalization winit's source does with its own line constant.
///
/// The sign is flipped for the same reason it is on the desktop: the DOM
/// reports how far the *content* moves, and the host takes how far the *view*
/// does.
pub fn wheel_delta_from_dom(event: &WheelEvent, line_px: f32, page_px: f32) -> (f32, f32) {
    let scale = match event.delta_mode() {
        WheelEvent::DOM_DELTA_LINE => line_px,
        WheelEvent::DOM_DELTA_PAGE => page_px,
        _ => 1.0,
    };
    (
        -(event.delta_x() as f32) * scale,
        -(event.delta_y() as f32) * scale,
    )
}

/// A DOM composition event in Cambium's neutral vocabulary.
///
/// The browser reports the same four states the host already has words for,
/// under different names: `compositionstart` enables, `compositionupdate`
/// carries preedit text, `compositionend` commits.
///
/// The DOM gives no selection range inside the preedit, so the caret offset is
/// `None` rather than guessed; a host that guessed would put the caret in the
/// wrong place in every script that composes.
pub fn composition_from_dom(kind: CompositionKind, data: String) -> CompositionEvent {
    match kind {
        CompositionKind::Start => CompositionEvent::Enabled,
        CompositionKind::Update => CompositionEvent::Preedit {
            text: data,
            selection: None,
        },
        CompositionKind::End => CompositionEvent::Commit(data),
    }
}

/// Which composition event arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositionKind {
    Start,
    Update,
    End,
}
