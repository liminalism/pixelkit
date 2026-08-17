//! pixelkit-shell: a winit + softbuffer host for software-rendered apps.
//!
//! Owns the window, paces redraws, translates input, tracks the scale
//! factor, and hands finished frames to a [`Presenter`]. Provenance:
//! `pos-client-ui::shell` (restaurant-pos), with HiDPI and the presenter seam
//! added; see `ATTRIBUTION.md`.

pub mod frame_log;
pub mod input;
pub mod present;
pub mod scale;
pub mod shell;

pub use frame_log::FrameTimer;
pub use input::Input;
pub use present::{PresentError, Presenter, PresenterBackend, SoftbufferPresenter};
pub use scale::Scale;
pub use shell::{run_app, KeyInput, Modifiers, MouseButton, PixelApp, Wake, Waker, WindowConfig};
