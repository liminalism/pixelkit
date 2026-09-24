//! The presenter seam: how a finished pixel buffer reaches the window.
//!
//! The application paints into a [`WindowBuffer`]; a [`Presenter`] owns the
//! platform surface and copies the buffer onto it. Today that is softbuffer.
//! A GPU presenter would implement the same trait — first as a texture upload
//! of the same buffer (vsync'd presents for free), later as a compositor if
//! profiling ever asks for one — without touching the application.

use std::num::NonZeroU32;
use std::sync::Arc;

use pixelkit_raster::WindowBuffer;
use softbuffer::{Context, Surface};
use winit::window::Window;

#[derive(Debug)]
pub enum PresentError {
    /// The platform surface refused; the message is the backend's.
    Backend(String),
    /// A zero-sized window; nothing to present to.
    InvalidSize,
}

impl std::fmt::Display for PresentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresentError::Backend(message) => write!(f, "presenter backend failed: {message}"),
            PresentError::InvalidSize => write!(f, "invalid surface size"),
        }
    }
}

impl std::error::Error for PresentError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenterBackend {
    Software,
    Gpu,
}

pub trait Presenter {
    /// The surface now has this physical size.
    fn resize(&mut self, width: u32, height: u32) -> Result<(), PresentError>;
    /// Copy the whole buffer to the window. `buffer` matches the last resize.
    fn present(&mut self, buffer: &WindowBuffer) -> Result<(), PresentError>;
    fn backend(&self) -> PresenterBackend;
}

/// softbuffer: a CPU buffer handed to the window system, no GPU involved.
pub struct SoftbufferPresenter {
    surface: Surface<Arc<Window>, Arc<Window>>,
    size: (u32, u32),
}

impl SoftbufferPresenter {
    pub fn new(window: Arc<Window>) -> Result<SoftbufferPresenter, PresentError> {
        let context =
            Context::new(window.clone()).map_err(|e| PresentError::Backend(e.to_string()))?;
        let surface =
            Surface::new(&context, window).map_err(|e| PresentError::Backend(e.to_string()))?;
        Ok(SoftbufferPresenter {
            surface,
            size: (0, 0),
        })
    }
}

impl Presenter for SoftbufferPresenter {
    fn resize(&mut self, width: u32, height: u32) -> Result<(), PresentError> {
        if self.size == (width, height) {
            return Ok(());
        }
        let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return Err(PresentError::InvalidSize);
        };
        self.surface
            .resize(w, h)
            .map_err(|e| PresentError::Backend(e.to_string()))?;
        self.size = (width, height);
        Ok(())
    }

    fn present(&mut self, buffer: &WindowBuffer) -> Result<(), PresentError> {
        if (buffer.width, buffer.height) != self.size {
            self.resize(buffer.width, buffer.height)?;
        }
        let mut frame = self
            .surface
            .buffer_mut()
            .map_err(|e| PresentError::Backend(e.to_string()))?;
        frame.copy_from_slice(&buffer.pixels);
        frame
            .present()
            .map_err(|e| PresentError::Backend(e.to_string()))
    }

    fn backend(&self) -> PresenterBackend {
        PresenterBackend::Software
    }
}
