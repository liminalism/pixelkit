//! A dropdown/select: a closed field that, clicked, opens a list of options
//! drawn after everything else in the frame.
//!
//! The same problem [`Tooltip`](crate::Tooltip) solves, solved the same way.
//! Immediate mode has no z-order, so a widget cannot draw its own popup
//! inline — whatever the screen draws next would cover it. [`Ui::dropdown`]
//! draws the closed field and records what to show; [`Dropdown::draw`] draws
//! the open list, once, after everything else, and reports which option (if
//! any) was picked so the screen can apply it to its own selection state.

use crate::widget::Ui;
use pixelkit_raster::Rect;
use pixelkit_shell::KeyInput;
use pixelkit_text::Align;

/// A dropdown's open/closed state, owned by the screen alongside whatever
/// `usize` holds the selected option.
#[derive(Debug, Default)]
pub struct Dropdown {
    open: Option<Open>,
}

#[derive(Debug)]
struct Open {
    /// Physical rect of the closed field, so the popup knows where to hang.
    field: Rect,
    options: Vec<String>,
}

impl Dropdown {
    pub fn new() -> Dropdown {
        Dropdown::default()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// Draw the open popup, if there is one, inside `bounds`, and report
    /// which option was clicked this frame. Call once, last, after
    /// everything else the screen draws. Closes itself on a pick, on
    /// Escape, or on a click that lands inside `bounds` but on none of the
    /// options — the same "click outside dismisses it" a real menu has.
    pub fn draw(&mut self, ui: &mut Ui<'_>, bounds: Rect) -> Option<usize> {
        let Some(open) = &self.open else {
            return None;
        };
        let theme = ui.theme;
        let row_h = theme.row_height;
        let total_h = row_h * open.options.len() as i32;
        let below = Rect::new(open.field.x, open.field.bottom(), open.field.w, total_h);
        let fits_above = open.field.y - total_h >= bounds.y;
        let rect = if below.bottom() > bounds.bottom() && fits_above {
            Rect::new(open.field.x, open.field.y - total_h, open.field.w, total_h)
        } else {
            below
        };

        ui.painter.push_clip(bounds);
        ui.painter
            .shadow_rect(rect, ui.px(2), ui.px(3), 0x0010_1010, 40);
        ui.painter.rounded_rect(
            rect,
            theme.corner_radius,
            theme.border_width,
            theme.panel,
            theme.panel_edge,
        );

        let mut picked = None;
        for (index, label) in open.options.iter().enumerate() {
            let row = Rect::new(rect.x, rect.y + row_h * index as i32, rect.w, row_h);
            if ui.input.hovering(row) {
                ui.painter.fill_rect(row, theme.hover);
            }
            ui.label(row.inset(theme.padding / 2), label, theme.text, Align::Left);
            if ui.input.take_click(row) {
                picked = Some(index);
            }
        }
        ui.painter.pop_clip();

        // Whatever click did not land on a row — inside `bounds` but
        // elsewhere — dismisses the popup rather than being left pending
        // for something behind it to react to next frame.
        let dismissed_by_click = ui.input.take_click(bounds);
        let dismissed_by_escape = ui
            .input
            .keys()
            .iter()
            .any(|key| matches!(key, KeyInput::Escape));
        if picked.is_some() || dismissed_by_click || dismissed_by_escape {
            self.open = None;
        }
        picked
    }
}

impl Ui<'_> {
    /// A closed field showing `options[selected]`. Returns whether it was
    /// clicked (opening or closing the popup) — the popup's contents are
    /// drawn later, by [`Dropdown::draw`].
    pub fn dropdown(
        &mut self,
        dropdown: &mut Dropdown,
        area: Rect,
        options: &[&str],
        selected: usize,
    ) -> bool {
        let theme = self.theme;
        let open = dropdown.is_open();
        let hovered = self.input.hovering(area);
        let fill = if open {
            theme.selection
        } else if hovered {
            theme.hover
        } else {
            theme.panel
        };
        self.painter.rounded_rect(
            area,
            theme.corner_radius,
            theme.border_width,
            fill,
            theme.panel_edge,
        );

        let chevron_w = self.px(20);
        let (chevron_area, label_area) = area.inset(theme.padding / 2).split_right(chevron_w);
        let label = options.get(selected).copied().unwrap_or("");
        self.label(label_area, label, theme.text, Align::Left);
        self.draw_chevron(chevron_area, open, theme.text_dim);

        let clicked = self.input.take_click(area);
        if clicked {
            if open {
                dropdown.close();
            } else {
                dropdown.open = Some(Open {
                    field: area,
                    options: options.iter().map(|s| s.to_string()).collect(),
                });
            }
        }
        clicked
    }

    /// A small up/down chevron, `open` picking which way it points.
    fn draw_chevron(&mut self, area: Rect, open: bool, colour: u32) {
        let cx = area.x as f32 + area.w as f32 / 2.0;
        let cy = area.y as f32 + area.h as f32 / 2.0;
        let s = self.scale.f(4.0);
        let points = if open {
            [
                [cx - s, cy + s * 0.4],
                [cx, cy - s * 0.5],
                [cx + s, cy + s * 0.4],
            ]
        } else {
            [
                [cx - s, cy - s * 0.4],
                [cx, cy + s * 0.5],
                [cx + s, cy - s * 0.4],
            ]
        };
        self.painter
            .stroke_polyline_aa(self.kernel, &points, self.scale.f(1.5), colour, 255);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::Theme;
    use pixelkit_raster::{Painter, RasterKernel, WindowBuffer};
    use pixelkit_shell::{Input, MouseButton, Scale};
    use pixelkit_text::font::test_fonts::set;
    use pixelkit_text::TextCache;

    struct Harness {
        buffer: WindowBuffer,
        text: TextCache,
        kernel: RasterKernel,
        input: Input,
    }

    impl Harness {
        fn new() -> Harness {
            Harness {
                buffer: WindowBuffer::new(200, 200),
                text: TextCache::new(set()),
                kernel: RasterKernel::new(),
                input: Input::new(),
            }
        }

        fn frame<R>(&mut self, run: impl FnOnce(&mut Ui<'_>) -> R) -> R {
            self.buffer.pixels.fill(0);
            let mut ui = Ui::new(
                Painter::new(&mut self.buffer),
                &mut self.text,
                &mut self.input,
                Theme::default(),
                Scale::ONE,
                &mut self.kernel,
            );
            run(&mut ui)
        }

        fn click(&mut self, x: f32, y: f32) {
            self.input.cursor_moved(x, y);
            self.input.mouse(MouseButton::Left, true);
        }
    }

    fn options() -> Vec<&'static str> {
        vec!["Light", "Dark", "System"]
    }

    fn field() -> Rect {
        Rect::new(10, 10, 100, 24)
    }

    #[test]
    fn clicking_the_closed_field_opens_it() {
        let mut h = Harness::new();
        let items = options();
        h.click(50.0, 20.0);
        let clicked = h.frame(|ui| {
            let mut dropdown = Dropdown::new();
            let clicked = ui.dropdown(&mut dropdown, field(), &items, 0);
            assert!(dropdown.is_open());
            clicked
        });
        assert!(clicked);
    }

    #[test]
    fn drawing_a_closed_dropdown_reports_nothing_picked() {
        let mut h = Harness::new();
        let mut dropdown = Dropdown::new();
        let picked = h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));
        assert_eq!(picked, None);
    }

    #[test]
    fn picking_an_option_reports_it_and_closes_the_popup() {
        let mut h = Harness::new();
        let items = options();
        let mut dropdown = Dropdown::new();

        h.click(50.0, 20.0);
        h.frame(|ui| ui.dropdown(&mut dropdown, field(), &items, 0));
        h.input.end_frame();

        // The popup hangs below the field; click its second row.
        let row_h = Theme::default().row_height;
        h.click(
            field().x as f32 + 5.0,
            (field().bottom() + row_h + 2) as f32,
        );
        let picked = h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));

        assert_eq!(picked, Some(1));
        assert!(!dropdown.is_open());
    }

    #[test]
    fn clicking_inside_bounds_but_off_the_list_dismisses_it_without_a_pick() {
        let mut h = Harness::new();
        let items = options();
        let mut dropdown = Dropdown::new();

        h.click(50.0, 20.0);
        h.frame(|ui| ui.dropdown(&mut dropdown, field(), &items, 0));
        h.input.end_frame();

        h.click(190.0, 190.0); // far from the popup
        let picked = h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));

        assert_eq!(picked, None);
        assert!(!dropdown.is_open());
    }

    #[test]
    fn escape_closes_the_popup() {
        let mut h = Harness::new();
        let items = options();
        let mut dropdown = Dropdown::new();

        h.click(50.0, 20.0);
        h.frame(|ui| ui.dropdown(&mut dropdown, field(), &items, 0));
        h.input.end_frame();

        h.input.key(KeyInput::Escape);
        let picked = h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));
        assert_eq!(picked, None);
        assert!(!dropdown.is_open());
    }

    #[test]
    fn clicking_the_open_field_again_closes_it_without_reopening() {
        let mut h = Harness::new();
        let items = options();
        let mut dropdown = Dropdown::new();

        h.click(50.0, 20.0);
        h.frame(|ui| ui.dropdown(&mut dropdown, field(), &items, 0));
        assert!(dropdown.is_open());
        h.input.end_frame();

        h.click(50.0, 20.0); // the field again
        h.frame(|ui| ui.dropdown(&mut dropdown, field(), &items, 0));
        assert!(!dropdown.is_open());

        // With nothing open, draw is a no-op — the field's own click already
        // consumed the frame's pending click.
        let picked = h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));
        assert_eq!(picked, None);
    }

    #[test]
    fn a_field_near_the_bottom_opens_its_popup_upward() {
        let mut h = Harness::new();
        let items = options();
        let mut dropdown = Dropdown::new();
        let near_bottom = Rect::new(10, 180, 100, 15); // little room below

        h.click(50.0, 185.0);
        h.frame(|ui| ui.dropdown(&mut dropdown, near_bottom, &items, 0));
        h.input.end_frame();

        h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));
        // Something was painted above the field's top edge.
        let above = (0..near_bottom.y as usize)
            .any(|y| (0..200usize).any(|x| h.buffer.pixels[y * 200 + x] != 0));
        assert!(above, "the popup should have opened upward");
    }

    #[test]
    fn an_empty_options_list_does_not_panic() {
        let mut h = Harness::new();
        let mut dropdown = Dropdown::new();
        h.click(50.0, 20.0);
        h.frame(|ui| ui.dropdown(&mut dropdown, field(), &[], 0));
        h.input.end_frame();
        let picked = h.frame(|ui| dropdown.draw(ui, Rect::new(0, 0, 200, 200)));
        assert_eq!(picked, None);
    }
}
