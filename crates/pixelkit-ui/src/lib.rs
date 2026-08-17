//! pixelkit-ui: immediate-mode widgets over pixelkit-raster/-text/-shell.
//!
//! No widget tree and no identity map: a screen owns its widget state and
//! passes `&mut` to widget functions. Provenance: `pos-client-ui::widget`
//! (restaurant-pos), see `ATTRIBUTION.md`.

pub mod chrome;
pub mod list;
pub mod tooltip;
pub mod widget;

pub use chrome::ButtonStyle;
pub use list::{ScrollList, ScrollState};
pub use tooltip::TooltipStyle;
pub use tooltip::Tooltip;
pub use widget::{
    column_widths, navigate, Cell, CellAppearance, Column, Row, TableOutcome, TableState,
    TextFieldState, Theme, Ui, Width,
};
