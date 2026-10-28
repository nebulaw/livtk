//! livtk's design system (proposal 0003 §4): the tokens, themes and components every view of Liv
//! is drawn with. It knows nothing of tasks; the app maps its concepts onto it. It builds on
//! gpui-component, wrapped here so the app never imports it and its API changes stop at this
//! crate.

use gpui_kit::App;

/// Set up the design system. Call it once, before any window.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
}
