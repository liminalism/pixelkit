//! pixelkit-raster: a dependency-free software rasterizer.
//!
//! A `u32` XRGB pixel buffer, a clip-stack painter, an exact-area
//! anti-aliasing kernel that fills any closed path, a small path builder on
//! top of it, and a PNG writer for headless screenshots.
//!
//! Provenance: the painter and blend kernels come from `pos-client-ui` /
//! `pos-simd` (restaurant-pos), the coverage kernel from `thai-text`, itself a
//! trim of the lege-pdf CPU renderer. See `ATTRIBUTION.md`.

pub mod blend;
pub mod color;
pub mod kernel;
pub mod painter;
pub mod path;
pub mod png;
mod shapes;

pub use kernel::{CoverageBitmap, FillRule, RasterKernel};
pub use painter::{Painter, Rect, WindowBuffer};
pub use path::Path;
