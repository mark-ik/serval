//! The accessibility seam, and an honest account of what it does not do yet.
//!
//! # The assumption that was wrong
//!
//! The browser-delivery plan says a browser's accessibility "is the document
//! you already rendered", so the DOM discharges the duty AccessKit discharges
//! on the desktop. That is true of an ordinary web page. It is **not** true
//! here, and the difference is the whole of this module.
//!
//! Cambium presents through netrender onto a `<canvas>`. A canvas has no
//! accessible structure: to a screen reader it is one opaque graphic,
//! whatever is painted on it. So the browser needs exactly what the desktop
//! needs, a parallel tree projected from the layout and kept in step with it.
//! The difference is only what the tree is made of, DOM elements with ARIA
//! rather than AccessKit nodes.
//!
//! That is a real projection to build, equivalent in size to
//! `cambium-winit-a11y`'s, and it is not built here.
//!
//! # Why this is a documented gap rather than a plausible stub
//!
//! An implementation that projected a few roles would look like it worked and
//! would be worse than nothing: a reader would hear a partial, confidently
//! wrong account of the interface, and the gap would stop being visible to
//! anyone deciding what to fix. So this projects nothing, says so, and the
//! canvas carries the one honest signal available: a label naming the
//! application, and an `aria-hidden` container reserved for the projection
//! when it lands.
//!
//! Note what is *not* blocked. The host's accessibility machinery is already
//! neutral and already runs here: focus tracking, the request vocabulary, and
//! the routing that turns a reader's Click or Focus into the same paths a
//! pointer takes. What is missing is only the projection that would let a
//! reader see the tree in the first place.

use cambium_rootstock::{
    A11yRequest, Accessibility, LeafRegistry, NodeId, OwnedLayout, ScriptedDom,
};
use web_sys::HtmlCanvasElement;

/// The browser accessibility seam.
///
/// Implements the host's contract so the application runs and its focus and
/// action routing stay live. Projects no tree; see the module docs.
pub struct DomAccessibility {
    canvas: HtmlCanvasElement,
    announced: bool,
    label: String,
}

impl DomAccessibility {
    /// Attach to the canvas the application presents onto.
    ///
    /// `label` is what a reader is told the canvas is. It is not a substitute
    /// for the tree; it is the one true thing that can be said without one.
    pub fn new(canvas: HtmlCanvasElement, label: impl Into<String>) -> Self {
        Self {
            canvas,
            announced: false,
            label: label.into(),
        }
    }

    /// Mark the canvas up once, on the first frame that has a tree to describe.
    fn announce(&mut self) {
        if self.announced {
            return;
        }
        self.announced = true;
        // `img` rather than `application`: the canvas really is one graphic
        // until the projection exists, and claiming an interactive role would
        // promise a reader keyboard semantics that are not published yet.
        let _ = self.canvas.set_attribute("role", "img");
        let _ = self.canvas.set_attribute("aria-label", &self.label);
    }
}

impl Accessibility for DomAccessibility {
    fn sync(
        &mut self,
        _dom: &ScriptedDom,
        _layout: &OwnedLayout,
        _leaves: &mut LeafRegistry<u64>,
        _focus: Option<u64>,
    ) -> Vec<A11yRequest> {
        self.announce();
        // No tree is published, so no reader can have acted on one. Returning
        // empty is the truthful answer rather than a placeholder.
        Vec::new()
    }
}
