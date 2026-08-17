//! The windowing-neutral half of the Cambium application host.
//!
//! `cambium-genet-winit-host` grew as one crate because there was one event
//! source. A browser is a second one, and the machinery it would need is the
//! same machinery: retained layout over the runner's DOM, logical-coordinate
//! hit testing, input routing, focus and spatial navigation, frame pacing, and
//! accessibility projection. None of that is about winit; the winit host
//! carries only 64 winit references across 3544 lines.
//!
//! This crate is that machinery's new home. It owns no event loop, no window,
//! and no surface. An event-source adapter (winit today, DOM next) converts
//! platform events into the vocabulary here and drives it.
//!
//! ## What lives here first
//!
//! The input vocabulary, because it is the seam every other piece crosses. The
//! winit host's [`KeyPress`] already existed for a version of this reason: a
//! `winit::event::KeyEvent` cannot be constructed outside winit, so a host that
//! routed one could never be driven from a test. A host that routes one also
//! cannot be driven from a browser. Same defect, wider blast radius.

use genet_layout::{IncrementalLayout, VisualMovement};
use genet_scripted_dom::{NodeId, ScriptedDom};
use sprigging::LeafRegistry;

/// The host's clock.
///
/// Re-exported so every host and adapter reads time from one place. This is not
/// a stylistic preference: `std::time::Instant::now()` compiles for
/// `wasm32-unknown-unknown` and panics at runtime, so a browser host that used
/// std time would type-check, build, ship, and then die on its first animated
/// frame. Nothing in a compiler catches that.
pub use web_time::{Duration, Instant};

/// The surface's containing frame, as the neutral machinery needs it.
///
/// Deliberately five methods, not a window abstraction. The host asks a window
/// for fifteen things, and the split between them is not arbitrary: these five
/// have honest browser analogues, and the other ten are desktop window
/// management, which lives with the adapter that has a window to manage.
///
/// | here | winit | browser |
/// |---|---|---|
/// | `request_redraw` | `Window::request_redraw` | `requestAnimationFrame` |
/// | `inner_size` | `Window::inner_size` | the canvas's client size |
/// | `scale_factor` | `Window::scale_factor` | `devicePixelRatio` |
/// | `set_ime_allowed` | `Window::set_ime_allowed` | focus on the editing host |
/// | `set_ime_cursor_area` | `Window::set_ime_cursor_area` | the composition rect |
///
/// Maximize, minimize, drag, close, the window menu and visibility are absent
/// on purpose. A browser tab does not maximize itself, and pretending otherwise
/// would put a method on this trait that one implementation could only answer
/// by lying.
pub trait HostWindow {
    /// Ask for another frame.
    fn request_redraw(&self);

    /// The drawable size in physical pixels.
    fn inner_size(&self) -> (u32, u32);

    /// Physical pixels per logical pixel.
    fn scale_factor(&self) -> f64;

    /// Whether the platform should offer text input.
    fn set_ime_allowed(&self, allowed: bool);

    /// Where the composition window should sit, in logical coordinates.
    fn set_ime_cursor_area(&self, x: f64, y: f64, width: f64, height: f64);
}

/// What the host presents onto.
///
/// The present core beneath this is already windowing-neutral: a
/// [`RenderCore`](genet_render_host::RenderCore) boots wgpu and owns the
/// renderer, and a surface is created against whatever the platform hands it.
/// `SurfaceHost` is that pair for a winit window; the browser presenter
/// assembles the same pair against an `HtmlCanvasElement`, differing in one
/// line, the surface target.
///
/// So this trait exists to name the pair, not to abstract a difference. Only
/// four methods are required; the rest reach the core, which every
/// implementation has.
pub trait Surface {
    /// The present core: wgpu device, queue, and the netrender renderer.
    fn core(&self) -> &genet_render_host::RenderCore;

    /// The surface's texture format.
    fn format(&self) -> wgpu::TextureFormat;

    /// Reconfigure for a new size.
    fn resize(&mut self, width: u32, height: u32);

    /// Acquire the next backbuffer, or `None` when the surface is not
    /// presentable this frame.
    fn acquire(&self) -> Option<wgpu::SurfaceTexture>;

    /// The netrender renderer.
    fn renderer(&self) -> &netrender::Renderer {
        self.core().renderer()
    }

    /// The wgpu device backing the renderer.
    fn device(&self) -> &wgpu::Device {
        self.core().device()
    }

    /// The wgpu queue backing the renderer.
    fn queue(&self) -> &wgpu::Queue {
        self.core().queue()
    }
}

/// What a screen reader asked for, kept as the action it actually requested.
///
/// `Click` and `Focus` stay apart because collapsing them lies to the reader:
/// navigating a list with a virtual cursor issues `Focus`, and turning that into
/// a click activates every control the reader merely moves across. The host
/// routes `Click` through its activation path and `Focus` through `set_focus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yAction {
    /// Activate the element: the reader's equivalent of a pointer click.
    Click,
    /// Move focus to the element, without activating it.
    Focus,
}

/// One drained screen-reader request: which action, on which DOM node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yRequest {
    pub action: A11yAction,
    pub node: NodeId,
}

/// How a host publishes its accessible tree and collects what a reader asked
/// for.
///
/// This is the seam where the two event sources differ most, and the only one
/// that is not a translation. AccessKit builds a *parallel* tree that the
/// platform queries through a handle bound to a native window. A browser has no
/// such handle, and needs no parallel tree: the document the host already
/// rendered is the accessible tree, so the same duty is discharged by keeping
/// ARIA state on nodes that exist.
///
/// The signature is what both can honestly implement. Notably absent is the
/// window: the winit implementation holds its own, and a browser has none to
/// hold.
///
/// Returning requests rather than dispatching them keeps the routing decision
/// with the host, which owns the activation and focus paths, rather than
/// splitting it across two implementations that would then have to agree.
pub trait Accessibility {
    /// Publish the current tree and drain what the reader asked for since the
    /// last call.
    ///
    /// `focus` is the focused DOM node's opaque id, used as the tree's focus
    /// only when that node is really in the tree, so a stale id never points
    /// the reader at nothing.
    fn sync(
        &mut self,
        dom: &ScriptedDom,
        layout: &IncrementalLayout<NodeId>,
        leaves: &mut LeafRegistry<u64>,
        focus: Option<u64>,
    ) -> Vec<A11yRequest>;
}

/// A logical key, after the platform applied layout and modifiers.
///
/// Mirrors [`cambium::Key`]'s cases and adds [`Unidentified`](Key::Unidentified),
/// which Cambium's vocabulary has no room for because Cambium sits downstream
/// of this decision: by the time a key reaches a runner it has either resolved
/// to text or to a named key.
///
/// The host cannot assume that. Windows delivers injected text as `VK_PACKET`,
/// which surfaces as an unidentified key carrying text. On-screen keyboards,
/// keyboard remappers, and several assistive input tools all type that way, so
/// dropping the case would mean people using them cannot type at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// A key that produced text: the resolved character string.
    Character(String),
    /// A named, non-text key.
    Named(NamedKey),
    /// A key the platform could not name. It may still carry text; see
    /// [`KeyPress::injected_text`].
    Unidentified,
    /// A dead key: an accent awaiting composition. Never types on its own, even
    /// if the platform reported text alongside it. Kept distinct from
    /// [`Unidentified`](Key::Unidentified) so the injected-text path cannot
    /// swallow it; both winit and the DOM report this case.
    Dead,
}

/// The named keys host routing reads.
///
/// Deliberately the same set as [`cambium::NamedKey`], so lowering is total and
/// obvious. An event source that sees a named key outside this set lowers it to
/// [`Other`](NamedKey::Other) rather than inventing a case here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    Backspace,
    Enter,
    Tab,
    Escape,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    /// A named key this vocabulary does not special-case.
    Other,
}

impl From<NamedKey> for cambium::NamedKey {
    fn from(named: NamedKey) -> Self {
        match named {
            NamedKey::Backspace => Self::Backspace,
            NamedKey::Enter => Self::Enter,
            NamedKey::Tab => Self::Tab,
            NamedKey::Escape => Self::Escape,
            NamedKey::Space => Self::Space,
            NamedKey::ArrowLeft => Self::ArrowLeft,
            NamedKey::ArrowRight => Self::ArrowRight,
            NamedKey::ArrowUp => Self::ArrowUp,
            NamedKey::ArrowDown => Self::ArrowDown,
            NamedKey::Delete => Self::Delete,
            NamedKey::Home => Self::Home,
            NamedKey::End => Self::End,
            NamedKey::PageUp => Self::PageUp,
            NamedKey::PageDown => Self::PageDown,
            NamedKey::Other => Self::Other,
        }
    }
}

/// Modifier state at the time of a press.
///
/// `meta` is the platform command key: Command on macOS, Super or Windows
/// elsewhere. The field names match [`cambium::Modifiers`] so the lowering is a
/// field copy rather than a mapping anyone has to check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    /// No modifiers held.
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// Whether this press is a command chord rather than typing.
    ///
    /// Control or the platform command key. Not Alt: on several layouts AltGr
    /// is how ordinary characters are typed, and treating it as a chord would
    /// stop those layouts from producing text.
    pub fn is_command_chord(self) -> bool {
        self.ctrl || self.meta
    }
}

impl From<Modifiers> for cambium::Modifiers {
    fn from(mods: Modifiers) -> Self {
        Self {
            shift: mods.shift,
            ctrl: mods.ctrl,
            alt: mods.alt,
            meta: mods.meta,
        }
    }
}

/// A key press as the host routes it.
///
/// Constructible anywhere, which is the point: a keyboard-order receipt that
/// cannot run in `cargo test` is a receipt nobody collects, and a host whose
/// keyboard path names its event source cannot gain a second one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPress {
    /// The logical key, after the layout and modifiers the platform applied.
    pub key: Key,
    /// The text this press produces, when the platform reports any.
    ///
    /// Carried separately because `key` alone is not always enough; see
    /// [`Key::Unidentified`].
    pub text: Option<String>,
    /// Modifiers held at the time of the press.
    pub modifiers: Modifiers,
    /// Whether this is an auto-repeat rather than a fresh press.
    pub repeat: bool,
}

impl KeyPress {
    /// A press with no modifiers held.
    pub fn new(key: Key) -> Self {
        let text = match &key {
            Key::Character(c) => Some(c.clone()),
            _ => None,
        };
        Self {
            key,
            text,
            modifiers: Modifiers::NONE,
            repeat: false,
        }
    }

    /// A named-key press (Tab, Enter, ArrowLeft, and so on).
    pub fn named(named: NamedKey) -> Self {
        Self::new(Key::Named(named))
    }

    /// A press of a key that produced this text.
    pub fn character(text: impl Into<String>) -> Self {
        Self::new(Key::Character(text.into()))
    }

    /// Hold these modifiers for the press.
    #[must_use]
    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Mark this press as an auto-repeat.
    #[must_use]
    pub fn repeated(mut self) -> Self {
        self.repeat = true;
        self
    }

    /// The character this press should insert when the platform could not name
    /// the key but did report text.
    ///
    /// `None` for a named key, a control character, or a chord: a shortcut must
    /// not become typed text.
    pub fn injected_text(&self) -> Option<&str> {
        if !matches!(self.key, Key::Unidentified) {
            return None;
        }
        if self.modifiers.is_command_chord() {
            return None;
        }
        let text = self.text.as_deref()?;
        if text.is_empty() || text.chars().any(char::is_control) {
            return None;
        }
        Some(text)
    }

    /// Lower into the key event a runner consumes, or `None` for a press the
    /// tree should never see.
    ///
    /// The unidentified-with-text case is the assistive-input path and is the
    /// reason this is not a plain `From`: whether such a press becomes text
    /// depends on the modifiers held, so the decision belongs with the press
    /// rather than with the key.
    ///
    /// A `None` return is the host's "dropped: no runner key and no text" case,
    /// which is worth tracing where it happens.
    pub fn to_runner_key(&self) -> Option<cambium::KeyEvent> {
        let mods = self.modifiers.into();
        let mapped = match &self.key {
            Key::Character(s) => cambium::Key::Character(s.clone()),
            Key::Named(named) => cambium::Key::Named((*named).into()),
            Key::Unidentified => cambium::Key::Character(self.injected_text()?.to_string()),
            Key::Dead => return None,
        };
        Some(cambium::KeyEvent::with_mods(mapped, mods))
    }

    /// The caret movement this press means by default, or `None` for a key the
    /// host has no caret default for.
    ///
    /// `word` selects word-granularity movement, which callers derive from the
    /// modifiers their platform uses for it.
    pub fn caret_movement(&self, word: bool) -> Option<VisualMovement> {
        let Key::Named(named) = &self.key else {
            return None;
        };
        Some(match named {
            NamedKey::ArrowLeft if word => VisualMovement::PreviousWord,
            NamedKey::ArrowLeft => VisualMovement::PreviousCluster,
            NamedKey::ArrowRight if word => VisualMovement::NextWord,
            NamedKey::ArrowRight => VisualMovement::NextCluster,
            NamedKey::ArrowUp => VisualMovement::PreviousLine,
            NamedKey::ArrowDown => VisualMovement::NextLine,
            NamedKey::Home => VisualMovement::LineStart,
            NamedKey::End => VisualMovement::LineEnd,
            _ => return None,
        })
    }

    /// The direction this press points, if it is an arrow key.
    pub fn direction(&self) -> Option<Direction> {
        match &self.key {
            Key::Named(named) => Direction::from_named(*named),
            _ => None,
        }
    }
}

/// Which way an arrow key pointed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// The arrow key this direction comes from, if any.
    pub fn from_named(named: NamedKey) -> Option<Self> {
        Some(match named {
            NamedKey::ArrowUp => Self::Up,
            NamedKey::ArrowDown => Self::Down,
            NamedKey::ArrowLeft => Self::Left,
            NamedKey::ArrowRight => Self::Right,
            _ => return None,
        })
    }
}

/// A laid-out rect as this module reasons about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box2 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Box2 {
    /// The rect's centre point.
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// How much an off-axis offset costs relative to on-axis distance. Above 1 so a
/// control that is roughly in line wins over a nearer one well off to the side —
/// which is what "down" means to a person looking at a column.
const OFF_AXIS_COST: f32 = 2.0;

/// The penalty for a candidate that does not overlap the current element's band
/// at all. Large enough that any aligned candidate beats any unaligned one,
/// finite so that a lone diagonal neighbour is still reachable rather than
/// stranding focus.
const NO_OVERLAP_PENALTY: f32 = 10_000.0;

/// Score `candidate` as a move from `from` in `dir`, or `None` when it is not in
/// that direction at all.
///
/// The rule is the ordinary spatial-navigation heuristic: travel along the axis,
/// penalized by how far off-axis you have to go, and penalized much harder for
/// leaving the current element's band entirely. Lower is better.
pub fn score(from: Box2, candidate: Box2, dir: Direction) -> Option<f32> {
    let (fx, fy) = from.centre();
    let (cx, cy) = candidate.centre();
    // A hair of tolerance, so two controls on the same visual row do not count
    // as being above or below each other through rounding.
    const EPS: f32 = 0.5;
    let (along, off, overlaps) = match dir {
        Direction::Right => (
            cx - fx,
            (cy - fy).abs(),
            candidate.y < from.y + from.h && from.y < candidate.y + candidate.h,
        ),
        Direction::Left => (
            fx - cx,
            (cy - fy).abs(),
            candidate.y < from.y + from.h && from.y < candidate.y + candidate.h,
        ),
        Direction::Down => (
            cy - fy,
            (cx - fx).abs(),
            candidate.x < from.x + from.w && from.x < candidate.x + candidate.w,
        ),
        Direction::Up => (
            fy - cy,
            (cx - fx).abs(),
            candidate.x < from.x + from.w && from.x < candidate.x + candidate.w,
        ),
    };
    if along <= EPS {
        return None;
    }
    let penalty = if overlaps { 0.0 } else { NO_OVERLAP_PENALTY };
    Some(along + off * OFF_AXIS_COST + penalty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_press_carries_its_text() {
        let press = KeyPress::character("h");
        assert_eq!(press.text.as_deref(), Some("h"));
        assert_eq!(press.key, Key::Character("h".into()));
    }

    #[test]
    fn injected_text_only_for_unidentified_keys() {
        // The assistive-input path: unidentified key, real text, no chord.
        let injected = KeyPress {
            key: Key::Unidentified,
            text: Some("é".into()),
            modifiers: Modifiers::NONE,
            repeat: false,
        };
        assert_eq!(injected.injected_text(), Some("é"));

        // A named key never injects, even carrying text.
        let named = KeyPress {
            key: Key::Named(NamedKey::Enter),
            text: Some("\r".into()),
            modifiers: Modifiers::NONE,
            repeat: false,
        };
        assert_eq!(named.injected_text(), None);
    }

    #[test]
    fn a_chord_is_a_command_not_typing() {
        let chord = KeyPress {
            key: Key::Unidentified,
            text: Some("s".into()),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            repeat: false,
        };
        assert_eq!(chord.injected_text(), None);
    }

    #[test]
    fn alt_is_not_a_chord_because_altgr_types() {
        let altgr = KeyPress {
            key: Key::Unidentified,
            text: Some("@".into()),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            repeat: false,
        };
        assert_eq!(altgr.injected_text(), Some("@"));
    }

    #[test]
    fn control_characters_are_not_typed() {
        let control = KeyPress {
            key: Key::Unidentified,
            text: Some("\u{7}".into()),
            modifiers: Modifiers::NONE,
            repeat: false,
        };
        assert_eq!(control.injected_text(), None);
    }

    #[test]
    fn a_dead_key_never_types_even_carrying_text() {
        // Preserves the winit path's behaviour: `Key::Dead` produced no runner
        // key there and must not start typing here by falling into the
        // injected-text case.
        let dead = KeyPress {
            key: Key::Dead,
            text: Some("´".into()),
            modifiers: Modifiers::NONE,
            repeat: false,
        };
        assert_eq!(dead.injected_text(), None);
        assert!(dead.to_runner_key().is_none());
    }

    #[test]
    fn injected_text_reaches_the_runner_as_a_character() {
        let injected = KeyPress {
            key: Key::Unidentified,
            text: Some("é".into()),
            modifiers: Modifiers::NONE,
            repeat: false,
        };
        let lowered = injected.to_runner_key().expect("should lower");
        assert!(matches!(lowered.key, cambium::Key::Character(ref c) if c == "é"));
    }

    #[test]
    fn an_unidentified_key_with_no_text_is_dropped() {
        let bare = KeyPress {
            key: Key::Unidentified,
            text: None,
            modifiers: Modifiers::NONE,
            repeat: false,
        };
        assert!(bare.to_runner_key().is_none());
    }

    #[test]
    fn word_granularity_follows_the_caller() {
        let left = KeyPress::named(NamedKey::ArrowLeft);
        assert_eq!(
            left.caret_movement(false),
            Some(VisualMovement::PreviousCluster)
        );
        assert_eq!(
            left.caret_movement(true),
            Some(VisualMovement::PreviousWord)
        );
        assert_eq!(KeyPress::character("h").caret_movement(false), None);
    }

    #[test]
    fn arrows_resolve_to_directions() {
        assert_eq!(
            KeyPress::named(NamedKey::ArrowLeft).direction(),
            Some(Direction::Left)
        );
        assert_eq!(KeyPress::named(NamedKey::Enter).direction(), None);
    }
}

#[cfg(test)]
mod spatial_tests {
    use super::*;

    fn b(x: f32, y: f32, w: f32, h: f32) -> Box2 {
        Box2 { x, y, w, h }
    }

    /// A control directly below beats a nearer one off to the side: "down" means
    /// down the column you are looking at.
    #[test]
    fn alignment_beats_raw_distance() {
        let from = b(0.0, 0.0, 100.0, 40.0);
        let below = score(from, b(0.0, 60.0, 100.0, 40.0), Direction::Down).expect("in direction");
        let aside =
            score(from, b(300.0, 45.0, 100.0, 40.0), Direction::Down).expect("in direction");
        assert!(
            below < aside,
            "aligned {below} must beat off-to-the-side {aside}",
        );
    }

    /// Nothing behind you is a candidate.
    #[test]
    fn the_opposite_direction_is_never_a_candidate() {
        let from = b(0.0, 100.0, 100.0, 40.0);
        assert!(score(from, b(0.0, 0.0, 100.0, 40.0), Direction::Down).is_none());
        assert!(score(from, b(0.0, 0.0, 100.0, 40.0), Direction::Up).is_some());
    }

    /// A control on the same visual row is not "below" its neighbour, so Down
    /// from one column does not slide sideways.
    #[test]
    fn the_same_row_is_not_below() {
        let from = b(0.0, 0.0, 100.0, 40.0);
        assert!(score(from, b(120.0, 0.0, 100.0, 40.0), Direction::Down).is_none());
        assert!(score(from, b(120.0, 0.0, 100.0, 40.0), Direction::Right).is_some());
    }
}
