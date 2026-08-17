//! Widgets, without a widget tree.
//!
//! Immediate mode: the screen is rebuilt from scratch every frame, and a
//! widget is a function that draws itself and answers a question — was this
//! clicked, which tab is showing, which row is selected.
//!
//! # No identity system
//!
//! The usual cost of immediate mode is identity. A framework has to store
//! scroll offsets and cursor positions for callers it knows nothing about, so
//! it hashes a call path or a string into a map and hopes the hash is stable.
//! Every scheme has a failure: a counter shifts state to the wrong widget the
//! moment a conditional appears; a `&'static str` collides when two tables
//! share a screen; a call-path hash collides for every row of a loop.
//!
//! There is no such map here. A screen is a struct that owns its own widget
//! state — `scroll: ScrollState`, `selected: Option<CashSessionId>` — and the
//! widget functions take `&mut` to it. That is type-checked, has no collision
//! class at all, survives reordering, and makes every widget testable without
//! drawing anything: build the state, feed it input, assert on the state.
//!
//! It works because this is six screens somebody controls, not a framework for
//! callers who have not been written yet.
//!
//! # What is worth testing
//!
//! Behaviour, at the state level: scroll clamping at both ends, selection
//! surviving a resort, a click at a point landing on the right row given the
//! scroll offset, a text cursor that never lands inside a Thai cluster. Not
//! "pixel (417, 92) is #1a2b3c" — when that fails it says nothing about what
//! broke. Whether it *looks* right is what the golden screens are for.

use std::borrow::Cow;

use pixelkit_raster::{Painter, RasterKernel, Rect};
use pixelkit_shell::{Input, KeyInput, Scale};
use pixelkit_text::{Align, FaceId, TextCache, TextStyle};

/// Colours and sizes, in one place so a screen does not invent its own.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: u32,
    pub panel: u32,
    pub panel_alt: u32,
    pub panel_edge: u32,
    pub text: u32,
    pub text_dim: u32,
    /// For a figure that is bad news — short, negative, unexplained.
    pub alarm: u32,
    pub alarm_soft: u32,
    pub good: u32,
    pub good_soft: u32,
    pub accent: u32,
    pub accent_soft: u32,
    /// A row under the cursor.
    pub hover: u32,
    /// The selected row.
    pub selection: u32,
    pub row_height: i32,
    /// Body text.
    pub body: TextStyle,
    /// Headings.
    pub heading: TextStyle,
    pub padding: i32,
    pub corner_radius: i32,
    pub border_width: i32,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            // Dark, because a back office is often a corner of a kitchen with
            // a window behind the screen.
            background: 0x0016_1a1f,
            panel: 0x001e_242b,
            panel_alt: 0x001a_2026,
            panel_edge: 0x002c_343d,
            text: 0x00e8_edf2,
            text_dim: 0x0090_9aa5,
            alarm: 0x00e0_6c5f,
            alarm_soft: 0x003b_2928,
            good: 0x006f_c28a,
            good_soft: 0x0024_3a2d,
            accent: 0x004d_9fd6,
            accent_soft: 0x0021_3847,
            hover: 0x0027_2f38,
            selection: 0x0032_4a5e,
            row_height: 30,
            body: TextStyle::new(FaceId(0), 17.0),
            heading: TextStyle::new(FaceId(0), 22.0),
            padding: 10,
            corner_radius: 7,
            border_width: 1,
        }
    }
}

/// What a widget draws into, and what it asks about input.
pub struct Ui<'a> {
    pub painter: Painter<'a>,
    pub text: &'a mut TextCache,
    pub input: &'a mut Input,
    pub theme: Theme,
    /// Logical→physical; widgets that take logical sizes go through it.
    pub scale: Scale,
    /// Scratch for anti-aliased shapes.
    pub kernel: &'a mut RasterKernel,
}

impl<'a> Ui<'a> {
    pub fn new(
        painter: Painter<'a>,
        text: &'a mut TextCache,
        input: &'a mut Input,
        theme: Theme,
        scale: Scale,
        kernel: &'a mut RasterKernel,
    ) -> Ui<'a> {
        Ui {
            painter,
            text,
            input,
            theme,
            scale,
            kernel,
        }
    }

    /// Logical pixels to physical, through the frame's scale.
    #[inline]
    pub fn px(&self, logical: i32) -> i32 {
        self.scale.px(logical)
    }

    /// A rounded surface, returning the area inside its content padding.
    pub fn surface(&mut self, area: Rect, fill: u32, edge: u32) -> Rect {
        self.painter.rounded_rect(
            area,
            self.theme.corner_radius,
            self.theme.border_width,
            fill,
            edge,
        );
        area.inset(self.theme.padding)
    }

    /// A panel with a border, returning the area inside it.
    pub fn panel(&mut self, area: Rect) -> Rect {
        self.surface(area, self.theme.panel, self.theme.panel_edge)
    }

    /// One line of text, vertically centred in `area`.
    pub fn label(&mut self, area: Rect, value: &str, colour: u32, align: Align) {
        let style = self.theme.body;
        self.label_styled(area, value, style, colour, align);
    }

    /// One line of text in an explicit style, vertically centred in `area`.
    pub fn label_styled(
        &mut self,
        area: Rect,
        value: &str,
        style: TextStyle,
        colour: u32,
        align: Align,
    ) {
        let size = style;
        let height = self.text.line_height(size);
        let y = area.y + (area.h - height) / 2;
        self.text
            .draw_fitted(&mut self.painter, value, area, y, size, colour, align);
    }

    pub fn heading(&mut self, area: Rect, value: &str) {
        let size = self.theme.heading;
        let height = self.text.line_height(size);
        let y = area.y + (area.h - height) / 2;
        let colour = self.theme.text;
        self.text
            .draw_fitted(&mut self.painter, value, area, y, size, colour, Align::Left);
    }

    /// A clickable button. Returns whether it was pressed this frame.
    pub fn button(&mut self, area: Rect, label: &str) -> bool {
        let pressed = self.input.pressing(area);
        let hovered = self.input.hovering(area);
        let fill = if pressed {
            self.theme.selection
        } else if hovered {
            self.theme.hover
        } else {
            self.theme.panel
        };
        self.painter.rounded_rect(
            area,
            self.theme.corner_radius,
            self.theme.border_width,
            fill,
            self.theme.panel_edge,
        );
        self.label(area, label, self.theme.text, Align::Centre);
        self.input.take_click(area)
    }

    /// A row of tabs. Returns true if the selection changed.
    ///
    /// The state is a plain index owned by the screen, so a screen with two
    /// tab strips has two fields and no way for them to be confused.
    pub fn tabs(&mut self, selected: &mut usize, area: Rect, labels: &[&str]) -> bool {
        if labels.is_empty() {
            return false;
        }
        // An index that has outlived the list it indexes — a screen rebuilt
        // with fewer tabs — snaps back rather than panicking.
        *selected = (*selected).min(labels.len() - 1);

        let width = area.w / labels.len() as i32;
        let mut changed = false;
        for (index, label) in labels.iter().enumerate() {
            let tab = Rect::new(area.x + width * index as i32, area.y, width, area.h);
            let active = index == *selected;
            let fill = if active {
                self.theme.selection
            } else if self.input.hovering(tab) {
                self.theme.hover
            } else {
                self.theme.panel
            };
            self.painter.rounded_rect(
                tab.inset(2),
                self.theme.corner_radius,
                self.theme.border_width,
                fill,
                self.theme.panel_edge,
            );
            let colour = if active {
                self.theme.text
            } else {
                self.theme.text_dim
            };
            self.label(tab, label, colour, Align::Centre);
            if self.input.take_click(tab) && !active {
                *selected = index;
                changed = true;
            }
        }
        changed
    }

    /// A horizontal bar, for a share of a total.
    ///
    /// `fraction` is clamped, because a share computed from figures that do
    /// not quite agree should draw a full bar rather than run off the panel.
    pub fn bar(&mut self, area: Rect, fraction: f64, colour: u32) {
        self.painter
            .fill_rounded_rect(area, area.h / 2, self.theme.hover);
        let fraction = fraction.clamp(0.0, 1.0);
        let width = (area.w as f64 * fraction).round() as i32;
        if width > 0 {
            self.painter.fill_rounded_rect(
                Rect::new(area.x, area.y, width, area.h),
                area.h / 2,
                colour,
            );
        }
    }

    /// A compact status label with a quiet tinted surface.
    pub fn badge(&mut self, area: Rect, label: &str, text: u32, fill: u32, edge: u32) {
        let badge = area.inset(4);
        self.painter.rounded_rect(
            badge,
            (self.theme.corner_radius - 1).max(0),
            self.theme.border_width,
            fill,
            edge,
        );
        self.label(badge.inset(4), label, text, Align::Centre);
    }

    /// A labelled progress meter. The track and label are drawn without an
    /// off-screen layer; callers can put exact quantities in `label`.
    pub fn meter(&mut self, area: Rect, fraction: f64, label: &str, colour: u32) {
        let meter = area.inset(5);
        self.bar(meter, fraction, colour);
        self.label(area, label, self.theme.text, Align::Centre);
    }
}

// --- scrolling --------------------------------------------------------------

/// How far a region has been scrolled, and how far it may be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollState {
    /// Pixels hidden above the top of the viewport. Never negative.
    offset: i32,
}

impl ScrollState {
    pub fn new() -> ScrollState {
        ScrollState::default()
    }

    pub fn offset(&self) -> i32 {
        self.offset
    }

    /// The furthest this can scroll given a viewport and its content.
    ///
    /// Zero when everything fits, so a short list cannot be scrolled at all —
    /// content that drifts off the top and cannot be brought back is the most
    /// common bug in a hand-written scroll region.
    pub fn max_offset(viewport: i32, content: i32) -> i32 {
        (content - viewport).max(0)
    }

    /// Move by a delta and clamp. Returns the offset actually applied.
    pub fn scroll_by(&mut self, delta: i32, viewport: i32, content: i32) -> i32 {
        let before = self.offset;
        self.offset = (self.offset + delta).clamp(0, ScrollState::max_offset(viewport, content));
        self.offset - before
    }

    /// Bring a band of content into view, moving as little as possible.
    pub fn reveal(&mut self, top: i32, height: i32, viewport: i32, content: i32) {
        let max = ScrollState::max_offset(viewport, content);
        if top < self.offset {
            self.offset = top;
        } else if top + height > self.offset + viewport {
            self.offset = top + height - viewport;
        }
        self.offset = self.offset.clamp(0, max);
    }

    /// Clamp after the content changed underneath — a filter applied, a day
    /// with fewer rows. Without this a list scrolled to the bottom shows
    /// nothing at all when it gets shorter.
    pub fn clamp_to(&mut self, viewport: i32, content: i32) {
        self.offset = self
            .offset
            .clamp(0, ScrollState::max_offset(viewport, content));
    }
}

// --- tables -----------------------------------------------------------------

/// How wide a column is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// Exactly this many pixels — a quantity, a price, a date.
    Fixed(i32),
    /// A share of whatever is left, by weight. Names take these.
    Flex(i32),
}

#[derive(Debug, Clone)]
pub struct Column {
    pub title: Cow<'static, str>,
    pub width: Width,
    pub align: Align,
}

impl Column {
    pub fn fixed(title: &'static str, width: i32, align: Align) -> Column {
        Column {
            title: Cow::Borrowed(title),
            width: Width::Fixed(width),
            align,
        }
    }

    pub fn flex(title: &'static str, weight: i32, align: Align) -> Column {
        Column {
            title: Cow::Borrowed(title),
            width: Width::Flex(weight),
            align,
        }
    }
}

/// How a table cell communicates its value.
#[derive(Debug, Clone, Copy)]
pub enum CellAppearance {
    Text,
    Badge { fill: u32, edge: u32 },
    Meter { fraction: f64, fill: u32 },
}

/// One cell's text and, where it matters, its colour or compact visual.
#[derive(Debug, Clone)]
pub struct Cell<'a> {
    pub text: Cow<'a, str>,
    pub colour: Option<u32>,
    pub appearance: CellAppearance,
}

impl<'a> Cell<'a> {
    pub fn new(text: impl Into<Cow<'a, str>>) -> Cell<'a> {
        Cell {
            text: text.into(),
            colour: None,
            appearance: CellAppearance::Text,
        }
    }

    pub fn coloured(text: impl Into<Cow<'a, str>>, colour: u32) -> Cell<'a> {
        Cell {
            text: text.into(),
            colour: Some(colour),
            appearance: CellAppearance::Text,
        }
    }

    pub fn badge(text: impl Into<Cow<'a, str>>, colour: u32, fill: u32, edge: u32) -> Cell<'a> {
        Cell {
            text: text.into(),
            colour: Some(colour),
            appearance: CellAppearance::Badge { fill, edge },
        }
    }

    pub fn meter(text: impl Into<Cow<'a, str>>, fraction: f64, fill: u32) -> Cell<'a> {
        Cell {
            text: text.into(),
            colour: None,
            appearance: CellAppearance::Meter { fraction, fill },
        }
    }
}

/// A table's scroll and selection, owned by the screen that draws it.
#[derive(Debug, Clone, Default)]
pub struct TableState {
    pub scroll: ScrollState,
    /// The selected row's **key**, not its index.
    ///
    /// An index is wrong the moment the table is re-sorted or filtered: the
    /// selection silently moves to whatever row now sits at that position,
    /// which for a stock table means a manager acting on the wrong
    /// ingredient. A key follows its row.
    selected: Option<String>,
}

impl TableState {
    pub fn new() -> TableState {
        TableState::default()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn select(&mut self, key: Option<String>) {
        self.selected = key;
    }

    pub fn is_selected(&self, key: &str) -> bool {
        self.selected.as_deref() == Some(key)
    }

    /// Which row is selected, by position, if it is still present.
    pub fn selected_index(&self, keys: &[String]) -> Option<usize> {
        let selected = self.selected.as_deref()?;
        keys.iter().position(|key| key == selected)
    }

    /// Move the selection by `delta` rows, clamped to the ends.
    ///
    /// With nothing selected, a downward move selects the first row and an
    /// upward move the last — so arrowing into an unfocused table does
    /// something obvious from either direction.
    pub fn move_by(&mut self, delta: i32, keys: &[String]) {
        if keys.is_empty() {
            self.selected = None;
            return;
        }
        let next = match self.selected_index(keys) {
            Some(index) => (index as i32 + delta).clamp(0, keys.len() as i32 - 1) as usize,
            None if delta >= 0 => 0,
            None => keys.len() - 1,
        };
        self.selected = Some(keys[next].clone());
    }

    /// Forget a selection whose row is gone — a filter typed, a day changed.
    pub fn prune(&mut self, keys: &[String]) {
        if self
            .selected
            .as_deref()
            .is_some_and(|selected| !keys.iter().any(|key| key == selected))
        {
            self.selected = None;
        }
    }

    fn prune_keyed<'k>(&mut self, row_count: usize, key_at: &impl Fn(usize) -> &'k str) {
        if self
            .selected
            .as_deref()
            .is_some_and(|selected| !(0..row_count).any(|index| key_at(index) == selected))
        {
            self.selected = None;
        }
    }
}

/// A row, built only when it is about to be drawn.
pub struct Row<'a> {
    pub cells: Vec<Cell<'a>>,
}

/// Lay columns out across a width.
///
/// Fixed columns take what they ask for; flex columns share what is left in
/// proportion to their weights, with the remainder going to the last one so
/// the row ends exactly at the right edge rather than a pixel short.
pub fn column_widths(columns: &[Column], total: i32, gap: i32) -> Vec<i32> {
    if columns.is_empty() {
        return Vec::new();
    }
    let gaps = gap * (columns.len() as i32 - 1);
    let fixed: i32 = columns
        .iter()
        .filter_map(|column| match column.width {
            Width::Fixed(width) => Some(width),
            Width::Flex(_) => None,
        })
        .sum();
    let weights: i32 = columns
        .iter()
        .filter_map(|column| match column.width {
            Width::Flex(weight) => Some(weight.max(0)),
            Width::Fixed(_) => None,
        })
        .sum();
    let spare = (total - gaps - fixed).max(0);

    let mut widths: Vec<i32> = columns
        .iter()
        .map(|column| match column.width {
            Width::Fixed(width) => width.max(0),
            Width::Flex(weight) if weights > 0 => spare * weight.max(0) / weights,
            Width::Flex(_) => 0,
        })
        .collect();

    // The remainder, to the last flexible column, so the row ends exactly at
    // the right edge rather than a pixel short of it.
    //
    // Clamped at zero: when the panel is narrower than its fixed columns and
    // gaps there is nothing to distribute, and the correction would be
    // negative. A window dragged very small should crowd its columns, not
    // hand one a negative width for some rectangle to trip over later.
    if weights > 0 {
        let assigned: i32 = widths.iter().sum::<i32>() + gaps;
        let last_flex = columns
            .iter()
            .rposition(|column| matches!(column.width, Width::Flex(_)));
        if let Some(index) = last_flex {
            widths[index] = (widths[index] + total - assigned).max(0);
        }
    }
    widths
}

/// The gap between one column and the next.
pub const COLUMN_GAP: i32 = 8;

/// Reserved down a table's right edge for the scrollbar.
///
/// Always, even when the list fits and no bar is drawn. Taking it only when
/// there is something to scroll would shift every column sideways the moment a
/// row was added, and would let the bar sit on top of the last column — which
/// reads as a rendering fault, because what it actually does is shave the last
/// digit off a price.
pub const TABLE_GUTTER: i32 = 10;

/// What a table did this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableOutcome {
    /// A row was clicked, and is now selected.
    pub activated: Option<String>,
    /// How many rows the viewport can show.
    pub visible_rows: i32,
}

impl<'a> Ui<'a> {
    /// Draw a scrolling, selectable table.
    ///
    /// Rows are produced by `row`, called only for the ones on screen — a
    /// thousand movements draw thirty. `keys` must list every row in order,
    /// because selection and keyboard movement are by key rather than index.
    pub fn table<'k>(
        &mut self,
        state: &mut TableState,
        area: Rect,
        columns: &[Column],
        keys: &'k [String],
        row: impl FnMut(usize) -> Row<'k>,
    ) -> TableOutcome {
        self.table_keyed(
            state,
            area,
            columns,
            keys.len(),
            |index| keys[index].as_str(),
            row,
        )
    }

    /// Draw a table whose stable keys are borrowed directly from its data.
    ///
    /// This is the report-friendly form: a stock screen no longer has to
    /// clone every ingredient code into a temporary `Vec<String>` on every
    /// repaint merely to support selection.
    pub fn table_keyed<'k>(
        &mut self,
        state: &mut TableState,
        area: Rect,
        columns: &[Column],
        row_count: usize,
        key_at: impl Fn(usize) -> &'k str,
        mut row: impl FnMut(usize) -> Row<'k>,
    ) -> TableOutcome {
        let theme = self.theme;
        let content_width = (area.w - TABLE_GUTTER).max(0);
        let widths = column_widths(columns, content_width, COLUMN_GAP);
        let (header, body) = area.split_top(theme.row_height);

        // Header
        let mut x = area.x;
        for (column, width) in columns.iter().zip(&widths) {
            let cell = Rect::new(x, header.y, *width, header.h);
            self.label(cell, &column.title, theme.text_dim, column.align);
            x += width + COLUMN_GAP;
        }
        self.painter
            .horizontal_rule(area.x, header.bottom() - 1, area.w, theme.panel_edge);

        let content = row_count as i32 * theme.row_height;
        // Before anything is drawn: the list may have shrunk since last frame,
        // and a table scrolled past its own end shows nothing at all.
        state.scroll.clamp_to(body.h, content);
        state.prune_keyed(row_count, &key_at);

        let (_, scrolled) = self.input.take_scroll(body);
        if scrolled != 0.0 {
            // Negative wheel delta means the content moves up.
            state
                .scroll
                .scroll_by(-scrolled.round() as i32, body.h, content);
        }

        let offset = state.scroll.offset();
        let first = (offset / theme.row_height).max(0) as usize;
        let last = (((offset + body.h) / theme.row_height) as usize + 1).min(row_count);

        let mut outcome = TableOutcome {
            activated: None,
            visible_rows: body.h / theme.row_height,
        };

        self.painter.push_clip(body);
        #[allow(clippy::needless_range_loop)]
        // Indexed rather than iterated: the row closure is called by index,
        // and the range is the visible window rather than the whole slice.
        for index in first..last {
            let top = body.y + index as i32 * theme.row_height - offset;
            let line = Rect::new(body.x, top, body.w, theme.row_height);
            if !self.painter.is_visible(line) {
                continue;
            }

            let key = key_at(index);
            let selected = state.is_selected(key);
            // Hover is tested against the row clipped to the viewport, so the
            // half of a row hanging below the fold does not light up when the
            // cursor is over whatever is drawn beneath the table.
            let hit = line.intersect(body);
            if selected {
                self.painter.fill_rect(line, theme.selection);
            } else if self.input.hovering(hit) {
                self.painter.fill_rect(line, theme.hover);
            }

            // Built here, not before the loop: a table of a thousand rows
            // draws thirty, and materialising the rest would allocate a
            // string per cell per frame for content nobody can see.
            let drawn = row(index);
            let mut x = body.x;
            for ((column, width), cell) in columns.iter().zip(&widths).zip(drawn.cells.iter()) {
                let bounds = Rect::new(x, top, *width, theme.row_height);
                let colour = cell.colour.unwrap_or(theme.text);
                match cell.appearance {
                    CellAppearance::Text => {
                        self.label(bounds, &cell.text, colour, column.align);
                    }
                    CellAppearance::Badge { fill, edge } => {
                        self.badge(bounds, &cell.text, colour, fill, edge);
                    }
                    CellAppearance::Meter { fraction, fill } => {
                        self.meter(bounds, fraction, &cell.text, fill);
                    }
                }
                x += width + COLUMN_GAP;
            }

            if self.input.take_click(hit) {
                let key = key.to_owned();
                state.select(Some(key.clone()));
                outcome.activated = Some(key);
            }
        }
        self.painter.pop_clip();

        // A scrollbar, only when there is somewhere to scroll to.
        let max = ScrollState::max_offset(body.h, content);
        if max > 0 {
            let track = Rect::new(body.right() - TABLE_GUTTER + 3, body.y, 4, body.h);
            self.painter.fill_rect(track, theme.hover);
            let thumb_height = (body.h * body.h / content).max(20);
            let travel = body.h - thumb_height;
            let thumb_top = body.y + (travel * offset / max);
            self.painter.fill_rect(
                Rect::new(track.x, thumb_top, track.w, thumb_height),
                theme.text_dim,
            );
        }
        outcome
    }
}

/// Keyboard handling shared by any list: arrows, page keys, home and end.
///
/// Returns whether the selection moved, so a caller can scroll it into view.
pub fn navigate(state: &mut TableState, keys: &[String], page: i32, pressed: &[KeyInput]) -> bool {
    let before = state.selected().map(str::to_owned);
    for key in pressed {
        match key {
            KeyInput::Up => state.move_by(-1, keys),
            KeyInput::Down => state.move_by(1, keys),
            KeyInput::PageUp => state.move_by(-page.max(1), keys),
            KeyInput::PageDown => state.move_by(page.max(1), keys),
            KeyInput::Home => state.move_by(i32::MIN / 2, keys),
            KeyInput::End => state.move_by(i32::MAX / 2, keys),
            _ => {}
        }
    }
    before.as_deref() != state.selected()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("row-{index}")).collect()
    }

    // --- scrolling ------------------------------------------------------

    #[test]
    fn a_list_that_fits_cannot_be_scrolled() {
        // The commonest hand-written scroll bug: content drifts off the top
        // and cannot be brought back.
        let mut scroll = ScrollState::new();
        assert_eq!(scroll.scroll_by(500, 400, 200), 0);
        assert_eq!(scroll.offset(), 0);
        assert_eq!(ScrollState::max_offset(400, 200), 0);
    }

    #[test]
    fn scrolling_clamps_at_both_ends() {
        let mut scroll = ScrollState::new();
        assert_eq!(scroll.scroll_by(-100, 100, 500), 0, "already at the top");

        scroll.scroll_by(1_000, 100, 500);
        assert_eq!(scroll.offset(), 400, "the last screenful, not past it");

        scroll.scroll_by(50, 100, 500);
        assert_eq!(scroll.offset(), 400);

        scroll.scroll_by(-1_000, 100, 500);
        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn a_shorter_list_pulls_the_view_back() {
        // A filter typed while scrolled to the bottom. Without clamping, the
        // table shows blank space and looks broken.
        let mut scroll = ScrollState::new();
        scroll.scroll_by(1_000, 100, 900);
        assert_eq!(scroll.offset(), 800);

        scroll.clamp_to(100, 150);
        assert_eq!(scroll.offset(), 50);

        scroll.clamp_to(100, 40);
        assert_eq!(scroll.offset(), 0, "everything fits now");
    }

    #[test]
    fn revealing_moves_as_little_as_it_must() {
        let mut scroll = ScrollState::new();
        // Below the fold: scroll just far enough that it is the bottom row.
        scroll.reveal(300, 30, 100, 1_000);
        assert_eq!(scroll.offset(), 230);

        // Already visible: nothing moves.
        scroll.reveal(250, 30, 100, 1_000);
        assert_eq!(scroll.offset(), 230);

        // Above: to the top of it, not past.
        scroll.reveal(100, 30, 100, 1_000);
        assert_eq!(scroll.offset(), 100);
    }

    // --- selection ------------------------------------------------------

    #[test]
    fn a_selection_follows_its_row_through_a_resort() {
        // The reason selection is by key. With an index, re-sorting moves the
        // selection to whatever now sits at that position — and on a stock
        // screen that is a manager acting on the wrong ingredient.
        let sorted_by_name = vec!["beef".to_owned(), "pork".to_owned(), "rice".to_owned()];
        let sorted_by_value = vec!["rice".to_owned(), "beef".to_owned(), "pork".to_owned()];

        let mut state = TableState::new();
        state.select(Some("pork".to_owned()));
        assert_eq!(state.selected_index(&sorted_by_name), Some(1));
        assert_eq!(state.selected_index(&sorted_by_value), Some(2));
        assert_eq!(state.selected(), Some("pork"), "still the pork");
    }

    #[test]
    fn a_selection_whose_row_is_gone_is_forgotten() {
        let mut state = TableState::new();
        state.select(Some("pork".to_owned()));
        state.prune(&["beef".to_owned(), "rice".to_owned()]);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn moving_the_selection_stops_at_the_ends() {
        let keys = keys(3);
        let mut state = TableState::new();

        state.move_by(1, &keys);
        assert_eq!(state.selected(), Some("row-0"), "down selects the first");

        state.move_by(-1, &keys);
        assert_eq!(state.selected(), Some("row-0"), "and stays there");

        state.move_by(10, &keys);
        assert_eq!(state.selected(), Some("row-2"), "not past the end");
    }

    #[test]
    fn arrowing_up_into_an_unselected_table_lands_at_the_bottom() {
        let keys = keys(4);
        let mut state = TableState::new();
        state.move_by(-1, &keys);
        assert_eq!(state.selected(), Some("row-3"));
    }

    #[test]
    fn an_empty_table_has_nothing_to_select() {
        let mut state = TableState::new();
        state.select(Some("gone".to_owned()));
        state.move_by(1, &[]);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn the_navigation_keys_all_move_the_selection() {
        let keys = keys(50);
        let mut state = TableState::new();

        assert!(navigate(&mut state, &keys, 10, &[KeyInput::Down]));
        assert_eq!(state.selected(), Some("row-0"));

        navigate(&mut state, &keys, 10, &[KeyInput::PageDown]);
        assert_eq!(state.selected(), Some("row-10"));

        navigate(&mut state, &keys, 10, &[KeyInput::End]);
        assert_eq!(state.selected(), Some("row-49"));

        navigate(&mut state, &keys, 10, &[KeyInput::Home]);
        assert_eq!(state.selected(), Some("row-0"));

        assert!(
            !navigate(&mut state, &keys, 10, &[KeyInput::Up]),
            "already at the top, so nothing moved"
        );
        assert!(
            !navigate(&mut state, &keys, 10, &[KeyInput::Character('x')]),
            "a key the table does not use"
        );
    }

    #[test]
    fn several_keys_in_one_frame_all_apply() {
        // Held arrow keys repeat faster than the screen redraws.
        let keys = keys(20);
        let mut state = TableState::new();
        navigate(
            &mut state,
            &keys,
            5,
            &[KeyInput::Down, KeyInput::Down, KeyInput::Down],
        );
        assert_eq!(state.selected(), Some("row-2"));
    }

    // --- columns --------------------------------------------------------

    #[test]
    fn columns_fill_their_width_exactly() {
        // A row that ends a pixel short of the panel looks like a rendering
        // fault, and one that ends a pixel over spills into the scrollbar.
        let columns = vec![
            Column::fixed("code", 100, Align::Left),
            Column::flex("name", 2, Align::Left),
            Column::fixed("value", 90, Align::Right),
            Column::flex("note", 1, Align::Left),
        ];
        for total in [400, 401, 402, 743, 1_000] {
            let widths = column_widths(&columns, total, 8);
            let used: i32 = widths.iter().sum::<i32>() + 8 * 3;
            assert_eq!(used, total, "at {total}px");
            assert!(widths.iter().all(|width| *width >= 0));
        }
    }

    #[test]
    fn flexible_columns_share_in_proportion() {
        let columns = vec![
            Column::flex("a", 1, Align::Left),
            Column::flex("b", 3, Align::Left),
        ];
        let widths = column_widths(&columns, 408, 8);
        assert_eq!(widths, vec![100, 300]);
    }

    #[test]
    fn a_narrow_panel_does_not_produce_negative_columns() {
        // A window dragged very small. Columns get nothing rather than a
        // negative width that would panic a rectangle somewhere later.
        let columns = vec![
            Column::fixed("code", 100, Align::Left),
            Column::flex("name", 1, Align::Left),
        ];
        let widths = column_widths(&columns, 20, 8);
        assert!(widths.iter().all(|width| *width >= 0), "{widths:?}");
        // The fixed column keeps its size and crowds the panel; the flexible
        // one gets nothing, which is the honest answer at 20 pixels.
        assert_eq!(widths, vec![100, 0]);
    }

    #[test]
    fn no_columns_is_not_a_crash() {
        assert!(column_widths(&[], 500, 8).is_empty());
    }

    #[test]
    fn only_fixed_columns_take_exactly_what_they_asked_for() {
        let columns = vec![
            Column::fixed("a", 100, Align::Left),
            Column::fixed("b", 50, Align::Right),
        ];
        assert_eq!(column_widths(&columns, 500, 8), vec![100, 50]);
    }
}

// --- text entry -------------------------------------------------------------

/// An editable line of text, and where the caret sits in it.
///
/// The caret is a **byte offset**, and it is never allowed to land inside a
/// cluster. Thai stacks marks on their base — ก้ is two characters and one
/// letter — so a caret between them is a position no reader recognises, and
/// backspacing from it removes a tone mark and silently changes the word
/// rather than deleting anything the typist can see.
///
/// This is the same rule [`crate::text::TextCache::truncate`] follows for
/// cutting, applied to moving and deleting, and it is checked the same way:
/// exhaustively, at every position in a string full of awkward stacks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextFieldState {
    text: String,
    caret: usize,
}

impl TextFieldState {
    pub fn new() -> TextFieldState {
        TextFieldState::default()
    }

    pub fn with_text(text: impl Into<String>) -> TextFieldState {
        let text = text.into();
        TextFieldState {
            caret: text.len(),
            text,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.text.len();
    }

    /// The start of the cluster containing `at`: back off any combining marks.
    fn cluster_start(&self, mut at: usize) -> usize {
        at = at.min(self.text.len());
        while at > 0
            && self.text[at..]
                .chars()
                .next()
                .is_some_and(pixelkit_text::font::is_thai_combining)
        {
            at -= self.text[..at]
                .chars()
                .next_back()
                .map(char::len_utf8)
                .unwrap_or(1);
        }
        at
    }

    /// The end of the cluster starting at `at`: one base plus its marks.
    fn cluster_end(&self, at: usize) -> usize {
        let mut end = match self.text[at..].chars().next() {
            Some(character) => at + character.len_utf8(),
            None => return at,
        };
        while let Some(next) = self.text[end..].chars().next() {
            if !pixelkit_text::font::is_thai_combining(next) {
                break;
            }
            end += next.len_utf8();
        }
        end
    }

    pub fn move_left(&mut self) {
        if self.caret == 0 {
            return;
        }
        let previous = self.caret
            - self.text[..self.caret]
                .chars()
                .next_back()
                .map(char::len_utf8)
                .unwrap_or(1);
        self.caret = self.cluster_start(previous);
    }

    pub fn move_right(&mut self) {
        if self.caret >= self.text.len() {
            return;
        }
        self.caret = self.cluster_end(self.caret);
    }

    pub fn home(&mut self) {
        self.caret = 0;
    }

    pub fn end(&mut self) {
        self.caret = self.text.len();
    }

    pub fn insert(&mut self, character: char) {
        self.text.insert(self.caret, character);
        self.caret += character.len_utf8();
    }

    /// Delete backwards — the whole cluster, base and marks together.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let previous = self.caret
            - self.text[..self.caret]
                .chars()
                .next_back()
                .map(char::len_utf8)
                .unwrap_or(1);
        let start = self.cluster_start(previous);
        self.text.replace_range(start..self.caret, "");
        self.caret = start;
    }

    /// Delete forwards, likewise a whole cluster.
    pub fn delete(&mut self) {
        if self.caret >= self.text.len() {
            return;
        }
        let end = self.cluster_end(self.caret);
        self.text.replace_range(self.caret..end, "");
    }

    /// Apply a frame's keys. Returns whether the text changed.
    pub fn apply(&mut self, keys: &[KeyInput]) -> bool {
        let mut changed = false;
        for key in keys {
            match key {
                KeyInput::Character(character) if !character.is_control() => {
                    self.insert(*character);
                    changed = true;
                }
                KeyInput::Backspace if self.caret > 0 => {
                    self.backspace();
                    changed = true;
                }
                KeyInput::Delete if self.caret < self.text.len() => {
                    self.delete();
                    changed = true;
                }
                KeyInput::Left => self.move_left(),
                KeyInput::Right => self.move_right(),
                KeyInput::Home => self.home(),
                KeyInput::End => self.end(),
                _ => {}
            }
        }
        changed
    }
}

impl Ui<'_> {
    /// An editable field. Returns whether its text changed this frame.
    ///
    /// `focused` is the screen's own idea of what has focus — another piece of
    /// state a screen owns rather than the toolkit guessing at.
    pub fn text_field(
        &mut self,
        state: &mut TextFieldState,
        area: Rect,
        placeholder: &str,
        focused: bool,
    ) -> bool {
        let theme = self.theme;
        self.painter.rounded_rect(
            area,
            theme.corner_radius,
            theme.border_width,
            theme.background,
            if focused {
                theme.accent
            } else {
                theme.panel_edge
            },
        );

        let inner = area.inset(theme.padding / 2);
        if state.is_empty() && !focused {
            self.label(inner, placeholder, theme.text_dim, Align::Left);
        } else {
            self.label(inner, state.text(), theme.text, Align::Left);
        }

        if focused {
            // A caret drawn at the measured width of the text before it, so
            // it sits where the next glyph will land rather than at a guess.
            let before = &state.text()[..state.caret()];
            let offset = self.text.measure(before, theme.body);
            let height = self.text.line_height(theme.body);
            let top = inner.y + (inner.h - height) / 2;
            self.painter
                .fill_rect(Rect::new(inner.x + offset, top, 2, height), theme.accent);
        }

        if focused {
            self.input.consume_keys(|keys| state.apply(keys))
        } else {
            false
        }
    }
}

#[cfg(test)]
mod text_field_tests {
    use super::*;

    /// Every Thai cluster boundary in a string, as byte offsets.
    fn boundaries(text: &str) -> Vec<usize> {
        let mut offsets = vec![0];
        for (index, character) in text.char_indices() {
            if index > 0 && !pixelkit_text::font::is_thai_combining(character) {
                offsets.push(index);
            }
        }
        offsets.push(text.len());
        offsets.dedup();
        offsets
    }

    /// A word of each awkward shape: tone on an upper vowel, below-vowel with
    /// a tone above, sara am, and Latin mixed in.
    const AWKWARD: &str = "น้ำพริกเผาปุ๋ยก็ ABC 12";

    #[test]
    fn the_caret_only_ever_rests_on_a_cluster_boundary() {
        // Walking right from the start, then left from the end. A caret
        // between ก and its tone mark is a position no reader recognises,
        // and backspacing from it changes the word rather than deleting a
        // letter.
        let allowed = boundaries(AWKWARD);

        let mut field = TextFieldState::with_text(AWKWARD);
        field.home();
        let mut seen = vec![field.caret()];
        while field.caret() < AWKWARD.len() {
            let before = field.caret();
            field.move_right();
            assert!(field.caret() > before, "the caret stopped moving");
            assert!(
                allowed.contains(&field.caret()),
                "rightwards, caret landed at {} which is inside a cluster",
                field.caret()
            );
            seen.push(field.caret());
        }

        field.end();
        while field.caret() > 0 {
            let before = field.caret();
            field.move_left();
            assert!(field.caret() < before);
            assert!(
                allowed.contains(&field.caret()),
                "leftwards, caret landed at {} which is inside a cluster",
                field.caret()
            );
        }

        // And the two directions agree about where the stops are.
        assert_eq!(seen, allowed);
    }

    #[test]
    fn backspace_removes_a_letter_rather_than_a_mark() {
        // น้ำ is three characters and two letters. Deleting one character at
        // a time would leave น with no tone mark — a different word, and the
        // typist saw nothing disappear.
        let mut field = TextFieldState::with_text("น้ำ");
        assert_eq!(field.text().chars().count(), 3);

        field.backspace();
        assert_eq!(field.text(), "น้", "ำ is one letter");
        field.backspace();
        assert_eq!(field.text(), "", "and น้ is the other, mark and all");
    }

    #[test]
    fn delete_forwards_removes_a_whole_cluster_too() {
        let mut field = TextFieldState::with_text("ก็ข");
        field.home();
        field.delete();
        assert_eq!(field.text(), "ข", "ก and its mark went together");
        field.delete();
        assert_eq!(field.text(), "");
        field.delete();
        assert_eq!(field.text(), "", "and deleting past the end is harmless");
    }

    #[test]
    fn backspacing_from_the_start_is_harmless() {
        let mut field = TextFieldState::with_text("ข้าว");
        field.home();
        field.backspace();
        assert_eq!(field.text(), "ข้าว");
        assert_eq!(field.caret(), 0);
    }

    #[test]
    fn deleting_from_every_position_leaves_valid_text() {
        // Exhaustive, because the failure is silent: the string stays valid
        // UTF-8 and simply means something else.
        for start in boundaries(AWKWARD) {
            let mut field = TextFieldState::with_text(AWKWARD);
            field.home();
            while field.caret() < start {
                field.move_right();
            }
            field.backspace();
            // The caret must sit at a cluster boundary, which is about what
            // is to its *right*: a mark there has been separated from the
            // base it belongs to. A mark to its left is ordinary — that is
            // simply the end of a complete cluster, which น้ is.
            assert!(
                !field.text()[field.caret()..]
                    .chars()
                    .next()
                    .is_some_and(pixelkit_text::font::is_thai_combining),
                "backspacing at {start} left the caret inside a cluster: {:?}",
                field.text()
            );
            // Whatever remains must not begin with a combining mark, which
            // would be a mark with no base to attach to.
            assert!(
                !field
                    .text()
                    .chars()
                    .next()
                    .is_some_and(pixelkit_text::font::is_thai_combining),
                "backspacing at {start} left {:?} starting with a mark",
                field.text()
            );
        }
    }

    #[test]
    fn typing_a_thai_word_reads_back_as_it_was_typed() {
        let mut field = TextFieldState::new();
        for character in "ข้าวผัด".chars() {
            field.insert(character);
        }
        assert_eq!(field.text(), "ข้าวผัด");
        assert_eq!(field.caret(), field.text().len());
    }

    #[test]
    fn a_frame_of_keys_applies_in_order_and_reports_a_change() {
        let mut field = TextFieldState::new();
        let typed: Vec<KeyInput> = "rice".chars().map(KeyInput::Character).collect();
        assert!(field.apply(&typed));
        assert_eq!(field.text(), "rice");

        assert!(field.apply(&[KeyInput::Backspace, KeyInput::Backspace]));
        assert_eq!(field.text(), "ri");

        // Movement is not a change.
        assert!(!field.apply(&[KeyInput::Home, KeyInput::Right, KeyInput::End]));
        assert_eq!(field.text(), "ri");
    }

    #[test]
    fn control_characters_are_not_typed_into_the_field() {
        // Tab arrives with text "\t" on some platforms, and a literal tab in
        // a search box is a character nobody can see or delete.
        let mut field = TextFieldState::new();
        assert!(!field.apply(&[
            KeyInput::Character('\t'),
            KeyInput::Character('\n'),
            KeyInput::Enter,
            KeyInput::Escape,
        ]));
        assert!(field.is_empty());
    }

    #[test]
    fn inserting_in_the_middle_lands_where_the_caret_is() {
        let mut field = TextFieldState::with_text("ac");
        field.move_left();
        field.insert('b');
        assert_eq!(field.text(), "abc");
        assert_eq!(field.caret(), 2);
    }

    #[test]
    fn an_empty_field_moves_nowhere() {
        let mut field = TextFieldState::new();
        field.move_left();
        field.move_right();
        field.backspace();
        field.delete();
        assert!(field.is_empty());
        assert_eq!(field.caret(), 0);
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    use pixelkit_raster::WindowBuffer;
    use pixelkit_shell::MouseButton;
    use pixelkit_text::font::test_fonts::set;

    const ROW_HEIGHT: i32 = 30;

    fn keys(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("row-{index}")).collect()
    }

    fn columns() -> Vec<Column> {
        vec![
            Column::flex("name", 1, Align::Left),
            Column::fixed("value", 80, Align::Right),
        ]
    }

    /// Draw a table once at a fixed size, with the given input, and report
    /// what it did. The buffer is real, so the drawing code runs.
    fn draw(
        state: &mut TableState,
        input: &mut Input,
        keys: &[String],
        area: Rect,
    ) -> TableOutcome {
        let mut buffer = WindowBuffer::new(400, 400);
        let mut text = TextCache::new(set());
        let mut kernel = RasterKernel::new();
        let theme = Theme {
            row_height: ROW_HEIGHT,
            ..Theme::default()
        };
        let mut ui = Ui::new(
            Painter::new(&mut buffer),
            &mut text,
            input,
            theme,
            Scale::ONE,
            &mut kernel,
        );
        ui.table(state, area, &columns(), keys, |index| Row {
            cells: vec![Cell::new(format!("ข้าว {index}")), Cell::new("฿70.00")],
        })
    }

    #[test]
    fn clicking_a_row_selects_the_one_under_the_cursor() {
        // The area is at y=0 with a header of one row height, so the body
        // starts at 30 and the first row occupies 30..60.
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(20);
        let mut state = TableState::new();

        let mut input = Input::new();
        input.cursor_moved(50.0, 75.0); // 45px into the body: row 1
        input.mouse(MouseButton::Left, true);

        let outcome = draw(&mut state, &mut input, &keys, area);
        assert_eq!(outcome.activated.as_deref(), Some("row-1"));
        assert_eq!(state.selected(), Some("row-1"));
    }

    #[test]
    fn a_click_lands_on_the_right_row_after_scrolling() {
        // The bug this exists for: hit-testing that ignores the scroll offset
        // selects a different row from the one under the cursor, and only
        // once the list has been scrolled — so it survives every test that
        // starts at the top.
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(50);
        let mut state = TableState::new();

        // Scroll down by four rows, then click the first visible one.
        let mut input = Input::new();
        input.cursor_moved(50.0, 100.0);
        input.scrolled(0.0, -(4.0 * ROW_HEIGHT as f32));
        draw(&mut state, &mut input, &keys, area);
        input.end_frame();
        assert_eq!(state.scroll.offset(), 4 * ROW_HEIGHT);

        let mut input = Input::new();
        input.cursor_moved(50.0, 45.0); // 15px into the body
        input.mouse(MouseButton::Left, true);
        let outcome = draw(&mut state, &mut input, &keys, area);

        assert_eq!(
            outcome.activated.as_deref(),
            Some("row-4"),
            "the row under the cursor, not the fourth from the top of the data"
        );
    }

    #[test]
    fn clicking_below_the_last_row_selects_nothing() {
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(2);
        let mut state = TableState::new();

        let mut input = Input::new();
        input.cursor_moved(50.0, 250.0);
        input.mouse(MouseButton::Left, true);

        let outcome = draw(&mut state, &mut input, &keys, area);
        assert_eq!(outcome.activated, None);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn a_click_on_the_header_is_not_a_click_on_a_row() {
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(10);
        let mut state = TableState::new();

        let mut input = Input::new();
        input.cursor_moved(50.0, 10.0);
        input.mouse(MouseButton::Left, true);

        assert_eq!(draw(&mut state, &mut input, &keys, area).activated, None);
    }

    #[test]
    fn a_table_of_a_thousand_rows_builds_only_what_it_draws() {
        // The whole reason rows come from a closure. Building a thousand
        // rows' worth of strings per frame is what makes a software-rendered
        // table unusable on a Pi.
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(1_000);
        let mut state = TableState::new();
        let mut input = Input::new();

        let mut buffer = WindowBuffer::new(400, 400);
        let mut text = TextCache::new(set());
        let mut kernel = RasterKernel::new();
        let theme = Theme {
            row_height: ROW_HEIGHT,
            ..Theme::default()
        };
        let mut built = 0usize;
        {
            let mut ui = Ui::new(
                Painter::new(&mut buffer),
                &mut text,
                &mut input,
                theme,
                Scale::ONE,
                &mut kernel,
            );
            ui.table(&mut state, area, &columns(), &keys, |_index| {
                built += 1;
                Row {
                    cells: vec![Cell::new("ข้าว"), Cell::new("฿70.00")],
                }
            });
        }
        // A 300px body at 30px a row is ten, plus one straddling the edge.
        assert!(built <= 12, "built {built} rows to draw about ten");
        assert!(built >= 10, "built {built}, which cannot fill the viewport");
    }

    #[test]
    fn scrolling_stops_at_the_bottom_of_the_data() {
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(15);
        let mut state = TableState::new();

        let mut input = Input::new();
        input.cursor_moved(50.0, 100.0);
        input.scrolled(0.0, -10_000.0);
        draw(&mut state, &mut input, &keys, area);

        // 15 rows of 30 is 450; a 300px body leaves 150 to scroll.
        assert_eq!(state.scroll.offset(), 150);
    }

    #[test]
    fn a_table_that_fits_does_not_scroll_at_all() {
        let area = Rect::new(0, 0, 300, 330);
        let keys = keys(3);
        let mut state = TableState::new();

        let mut input = Input::new();
        input.cursor_moved(50.0, 100.0);
        input.scrolled(0.0, -500.0);
        draw(&mut state, &mut input, &keys, area);

        assert_eq!(state.scroll.offset(), 0);
    }

    #[test]
    fn a_selection_that_leaves_the_data_is_dropped_when_the_table_redraws() {
        let area = Rect::new(0, 0, 300, 330);
        let mut state = TableState::new();
        state.select(Some("row-40".to_owned()));

        let mut input = Input::new();
        draw(&mut state, &mut input, &keys(5), area);
        assert_eq!(state.selected(), None, "a filter removed that row");
    }

    #[test]
    fn an_empty_table_draws_without_complaint() {
        let area = Rect::new(0, 0, 300, 330);
        let mut state = TableState::new();
        let mut input = Input::new();
        let outcome = draw(&mut state, &mut input, &[], area);
        assert_eq!(outcome.activated, None);
        assert_eq!(state.scroll.offset(), 0);
    }

    #[test]
    fn rows_are_clipped_to_the_table_rather_than_drawn_over_what_is_below() {
        // A row half past the bottom edge must be cut, not painted across
        // whatever panel comes next.
        let mut buffer = WindowBuffer::new(400, 400);
        let mut text = TextCache::new(set());
        let mut kernel = RasterKernel::new();
        let mut input = Input::new();
        let mut state = TableState::new();
        let keys = keys(50);
        let area = Rect::new(0, 0, 300, 200);
        state.select(Some("row-40".to_owned()));

        {
            let theme = Theme {
                row_height: ROW_HEIGHT,
                ..Theme::default()
            };
            let mut ui = Ui::new(
                Painter::new(&mut buffer),
                &mut text,
                &mut input,
                theme,
                Scale::ONE,
                &mut kernel,
            );
            ui.table(&mut state, area, &columns(), &keys, |index| Row {
                cells: vec![Cell::new(format!("ข้าวสาร {index}")), Cell::new("฿70.00")],
            });
        }

        // Nothing below the table's own bounds was touched.
        let below = (area.bottom() as usize..400)
            .any(|y| (0..400).any(|x| buffer.pixels[y * 400 + x] != 0));
        assert!(!below, "the table drew past its own bottom edge");
    }
}

#[cfg(test)]
mod gutter_tests {
    use super::*;
    use pixelkit_raster::WindowBuffer;
    use pixelkit_text::font::test_fonts::set;

    /// A table always leaves room for its scrollbar, scrollable or not.
    ///
    /// The bug this fixes was visible rather than logical: the bar was drawn
    /// over the last column and shaved the final digit off every price, so
    /// ฿143.50 read as ฿143.5. Reserving only when scrollable would instead
    /// have shifted every column the moment a row was added.
    #[test]
    fn columns_never_reach_under_the_scrollbar() {
        let columns = vec![
            Column::flex("name", 1, Align::Left),
            Column::fixed("value", 120, Align::Right),
        ];
        for total in [300, 500, 743, 1_100] {
            let widths = column_widths(&columns, total - TABLE_GUTTER, COLUMN_GAP);
            let right_edge: i32 = widths.iter().sum::<i32>() + COLUMN_GAP;
            assert!(
                right_edge <= total - TABLE_GUTTER,
                "at {total}px the columns reach {right_edge}, into the gutter"
            );
        }
    }

    #[test]
    fn a_short_table_and_a_long_one_lay_out_their_columns_identically() {
        // Two frames of the same screen, one row apart. Columns that moved
        // between them would make a list twitch as it filled.
        let columns = vec![
            Column::flex("name", 1, Align::Left),
            Column::fixed("value", 120, Align::Right),
        ];
        let area = Rect::new(0, 0, 400, 200);
        let mut text = TextCache::new(set());
        let mut kernel = RasterKernel::new();

        let mut cell_left = Vec::new();
        for count in [2usize, 200usize] {
            let keys: Vec<String> = (0..count).map(|index| format!("k{index}")).collect();
            let mut buffer = WindowBuffer::new(500, 300);
            let mut input = Input::new();
            let mut state = TableState::new();
            let widths;
            {
                let mut ui = Ui::new(
                    Painter::new(&mut buffer),
                    &mut text,
                    &mut input,
                    Theme::default(),
                    Scale::ONE,
                    &mut kernel,
                );
                ui.table(&mut state, area, &columns, &keys, |_index| Row {
                    cells: vec![Cell::new("ข้าวสาร"), Cell::new("฿143.50")],
                });
                widths = column_widths(&columns, area.w - TABLE_GUTTER, COLUMN_GAP);
            }
            cell_left.push(widths);
        }
        assert_eq!(cell_left[0], cell_left[1]);
    }
}
