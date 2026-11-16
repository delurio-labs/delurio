//! The diagram as a canvas to move around, as in a design tool: it opens fitted to its box, a drag moves it (the hand),
//! Ctrl or ⌘ with the wheel, or a pinch, zooms at the pointer, and the wheel alone moves it until an edge, after which
//! the page scrolls on. Shift+1 fits it again, Shift+0 shows it at 100%, and "Full window" gives it the whole window.
//! Without scripts the page keeps its plain diagram, which scrolls sideways. The port of pan.ts: its two texts are kept
//! in assets/ as the TypeScript module gave them, byte for byte.

/// the canvas's styles
pub const PAN_CSS: &str = include_str!("../assets/pan.css");

/// plain JavaScript, run as the page loads; it reads no data and sends nothing
pub const PAN_SCRIPT: &str = include_str!("../assets/pan.html");
