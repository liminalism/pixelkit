//! pixelkit-windowing: windows at every level of a pixelkit application.
//!
//! - [`host`]: the OS window host — the event loop, [`PixelApp`], and the
//!   [`run_app`]/[`run_windows`] runners for one window or several.
//! - [`panes`]: tiled in-app panes with draggable dividers.
//! - [`windows`]: floating in-app windows that move, resize, raise and close.
//!
//! The in-app pieces are caller-owned state, like the rest of the toolkit's
//! immediate-mode widgets: the application keeps a [`Panes`] or [`Windows`]
//! as an ordinary field, calls `update` once a frame **before drawing** (so a
//! grabbed divider or title bar claims its press ahead of the content
//! underneath), then draws each pane or window from the rects it reports.
//!
//! The host moved here from `pixelkit-shell`; the panes and the floating
//! windows are new. Provenance of the host: `pos-client-ui::shell`
//! (restaurant-pos), see `ATTRIBUTION.md`.

pub mod host;
pub mod panes;
pub mod windows;

pub use host::{
    run_app, run_app_with_presenter, run_windows, run_windows_with_presenter, CursorShape,
    GesturePhase, ImeCursorArea, ImeEvent, KeyEvent, MultiPresenterFactory, PixelApp,
    PresenterFactory, ScrollEvent, Wake, Waker, WindowConfig, WindowRequest,
};
pub use panes::{PaneId, PaneMetrics, Panes, Side};
pub use windows::{WindowId, WindowMetrics, Windows};
