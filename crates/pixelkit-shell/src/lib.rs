//! pixelkit-shell: the pieces a window host and its application share.
//!
//! Per-frame [`Input`], logical-to-physical [`Scale`], the [`Presenter`]
//! seam that carries a finished buffer to the screen, frame timing and the
//! clipboard. Window hosting itself — the event loop,
//! `pixelkit_windowing::PixelApp`, `WindowConfig`, `run_app` — lives in
//! `pixelkit-windowing`.

pub mod clipboard;
pub mod frame_log;
pub mod input;
pub mod platform;
pub mod present;
pub mod scale;

pub use clipboard::Clipboard;
pub use frame_log::{FrameSnapshot, FrameTimer, PhaseStats};
pub use input::{Input, KeyInput, Modifiers, MouseButton};
pub use platform::{beep, system_accent};
pub use present::{PresentError, Presenter, PresenterBackend, SoftbufferPresenter};
pub use scale::Scale;
