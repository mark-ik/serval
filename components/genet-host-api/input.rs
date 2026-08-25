/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Host-neutral document input and the effects a content session may request.
//!
//! Platform adapters translate native events into these values. A host consumes
//! navigation and submission effects; a document engine never creates windows,
//! resolves policy, or replaces its own session.

/// Pointer buttons understood by document hosts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerButton {
    Primary,
    Auxiliary,
    Secondary,
    Other(u16),
}

/// Whether a pointer button changed to pressed or released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonState {
    Pressed,
    Released,
}

/// A portable keyboard key. `Character` keeps platform text separate from
/// command keys; text itself is delivered through [`HostInput::Text`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Space,
    Character(String),
    Other(String),
}

/// Keyboard modifiers at the time an input is delivered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Composition/IME lifecycle. The payload stays plain Unicode so document
/// engines do not learn a platform's IME types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextComposition {
    Started,
    Updated(String),
    Committed(String),
    Cancelled,
}

/// Focus transitions supplied by the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusChange {
    Gained,
    Lost,
    Next,
    Previous,
}

/// Browser commands whose interpretation belongs to the host's navigation
/// state rather than an engine-specific document session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NavigationCommand {
    Address(String),
    Reload,
    Stop,
    Back,
    Forward,
}

/// Input a host can deliver to a document surface.
#[derive(Clone, Debug, PartialEq)]
pub enum HostInput {
    PointerMoved {
        x: f32,
        y: f32,
    },
    PointerButton {
        button: PointerButton,
        state: ButtonState,
        x: f32,
        y: f32,
    },
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    },
    Key {
        key: HostKey,
        modifiers: InputModifiers,
    },
    Text(String),
    Composition(TextComposition),
    Focus(FocusChange),
    Navigation(NavigationCommand),
}

/// Cursor affordances a session may request without selecting a windowing API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorShape {
    Default,
    Text,
    Pointer,
    Wait,
}

/// A semantic result produced while handling [`HostInput`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostEffect {
    Redraw,
    Cursor(CursorShape),
    /// A document resolved a link. The host resolves it against its current
    /// address, applies trust/route policy, and replaces the live session.
    Navigate {
        target: String,
    },
    /// A document resolved a form endpoint. The host owns body collection,
    /// confirmation, transport, and the resulting navigation.
    Submit {
        target: String,
    },
}
