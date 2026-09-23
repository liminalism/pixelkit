//! pixelkit-shell: a winit + softbuffer host for software-rendered apps.
//!
//! Owns the window, paces redraws, translates input, tracks the scale
//! factor, and hands finished frames to a [`Presenter`]. Provenance:
//! `pos-client-ui::shell` (restaurant-pos), with HiDPI and the presenter seam
//! added; see `ATTRIBUTION.md`.

pub mod clipboard;
pub mod frame_log;
pub mod input;
pub mod platform;
pub mod present;
pub mod scale;
pub mod shell;

pub use clipboard::Clipboard;
pub use frame_log::{FrameSnapshot, FrameTimer, PhaseStats};
pub use input::Input;
pub use platform::{beep, system_accent};
pub use present::{PresentError, Presenter, PresenterBackend, SoftbufferPresenter};
pub use scale::Scale;
pub use shell::{
    run_app, run_app_with_presenter, GesturePhase, ImeCursorArea, ImeEvent, KeyEvent, KeyInput,
    Modifiers, MouseButton, PixelApp, PresenterFactory, ScrollEvent, Wake, Waker, WindowConfig,
};
