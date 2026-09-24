//! A frame's worth of input.
//!
//! The shell delivers events one at a time as they arrive; a screen wants to
//! know, while it is drawing, what happened since it last drew. This collects
//! the one into the other.
//!
//! The distinction that matters is between **state** and **edges**. Whether a
//! button is held down is state: it stays true across frames and is true for
//! whoever asks. That the button went down *this frame* is an edge, and it must
//! be consumed by exactly one widget or a single click activates every button
//! it happens to be over. So [`Input::take_click`] takes the click rather than
//! reading it, and [`Input::end_frame`] clears whatever nothing took.

use crate::shell::{KeyInput, Modifiers, MouseButton};

/// What happened since the last frame.
#[derive(Debug, Clone, Default)]
pub struct Input {
    /// Where the cursor is, in buffer pixels. `None` if it has left the window.
    cursor: Option<(i32, i32)>,
    /// Whether the left button is down right now.
    held: bool,
    /// A press that no widget has claimed yet.
    pending_click: Option<(i32, i32)>,
    /// A right-click that no widget has claimed yet.
    pending_context: Option<(i32, i32)>,
    /// Wheel movement, in pixels, not yet consumed.
    scroll: (f32, f32),
    /// Keys pressed this frame, in order.
    keys: Vec<KeyInput>,
    modifiers: Modifiers,
}

impl Input {
    pub fn new() -> Input {
        Input::default()
    }

    // --- what the shell calls -------------------------------------------

    pub fn cursor_moved(&mut self, x: f32, y: f32) {
        self.cursor = Some((x.round() as i32, y.round() as i32));
    }

    pub fn cursor_left(&mut self) {
        self.cursor = None;
        // A drag that ends outside the window never sends a release, so
        // holding this true would leave every widget thinking the button is
        // still down.
        self.held = false;
    }

    pub fn mouse(&mut self, button: MouseButton, pressed: bool) {
        match (button, pressed) {
            (MouseButton::Left, true) => {
                self.held = true;
                self.pending_click = self.cursor;
            }
            (MouseButton::Left, false) => self.held = false,
            (MouseButton::Right, true) => self.pending_context = self.cursor,
            _ => {}
        }
    }

    pub fn scrolled(&mut self, dx: f32, dy: f32) {
        self.scroll.0 += dx;
        self.scroll.1 += dy;
    }

    pub fn key(&mut self, key: KeyInput) {
        self.keys.push(key);
    }

    pub fn modifiers_changed(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    /// Drop anything no widget claimed. Called after each frame is drawn.
    ///
    /// Unclaimed input is discarded rather than carried forward: a click on
    /// empty space is a click on empty space, and holding it until something
    /// happens to be drawn there next frame would make the interface act on
    /// intentions nobody had.
    pub fn end_frame(&mut self) {
        self.pending_click = None;
        self.pending_context = None;
        self.scroll = (0.0, 0.0);
        self.keys.clear();
    }

    // --- what widgets ask -----------------------------------------------

    pub fn cursor(&self) -> Option<(i32, i32)> {
        self.cursor
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// Whether the cursor is inside a rectangle.
    pub fn hovering(&self, area: pixelkit_raster::Rect) -> bool {
        self.cursor.is_some_and(|(x, y)| area.contains(x, y))
    }

    /// Whether the button is held with the cursor inside a rectangle — for a
    /// widget that wants to look pressed.
    pub fn pressing(&self, area: pixelkit_raster::Rect) -> bool {
        self.held && self.hovering(area)
    }

    /// Claim a click inside `area`, if there is one to claim.
    ///
    /// Taking rather than reading is the whole design: a click belongs to one
    /// widget. Two overlapping widgets both reading it would both fire, and
    /// the second would usually be the one drawn underneath.
    pub fn take_click(&mut self, area: pixelkit_raster::Rect) -> bool {
        match self.pending_click {
            Some((x, y)) if area.contains(x, y) => {
                self.pending_click = None;
                true
            }
            _ => false,
        }
    }

    pub fn take_context_click(&mut self, area: pixelkit_raster::Rect) -> bool {
        match self.pending_context {
            Some((x, y)) if area.contains(x, y) => {
                self.pending_context = None;
                true
            }
            _ => false,
        }
    }

    /// Claim wheel movement over `area`.
    ///
    /// Returns `(dx, dy)` and clears them, so a scroll over nested regions
    /// moves the innermost one rather than both.
    pub fn take_scroll(&mut self, area: pixelkit_raster::Rect) -> (f32, f32) {
        if self.scroll == (0.0, 0.0) || !self.hovering(area) {
            return (0.0, 0.0);
        }
        std::mem::replace(&mut self.scroll, (0.0, 0.0))
    }

    /// Keys pressed this frame, for whatever has focus.
    pub fn keys(&self) -> &[KeyInput] {
        &self.keys
    }

    /// Claim the keys, so only the focused widget sees them.
    pub fn take_keys(&mut self) -> Vec<KeyInput> {
        std::mem::take(&mut self.keys)
    }

    /// Let one focused widget consume the key slice while retaining the
    /// collector's allocation for the next frame.
    ///
    /// `take_keys` is useful when ownership must leave the input object, but a
    /// text field only needs to inspect keys. Moving the `Vec` out there made
    /// every subsequent keystroke allocate a new buffer.
    pub fn consume_keys<R>(&mut self, consume: impl FnOnce(&[KeyInput]) -> R) -> R {
        let result = consume(&self.keys);
        self.keys.clear();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixelkit_raster::Rect;

    fn at(x: f32, y: f32) -> Input {
        let mut input = Input::new();
        input.cursor_moved(x, y);
        input
    }

    #[test]
    fn a_click_belongs_to_exactly_one_widget() {
        // Two widgets overlapping the same point. Without claiming, both fire
        // and the one drawn underneath usually wins.
        let mut input = at(10.0, 10.0);
        input.mouse(MouseButton::Left, true);

        let over = Rect::new(0, 0, 20, 20);
        assert!(input.take_click(over), "the first claimant gets it");
        assert!(!input.take_click(over), "and there is nothing left to take");
    }

    #[test]
    fn a_click_outside_is_not_claimed_and_stays_available() {
        let mut input = at(50.0, 50.0);
        input.mouse(MouseButton::Left, true);

        assert!(!input.take_click(Rect::new(0, 0, 20, 20)));
        assert!(
            input.take_click(Rect::new(40, 40, 20, 20)),
            "a widget that does contain it can still claim it"
        );
    }

    #[test]
    fn unclaimed_input_does_not_survive_the_frame() {
        // A click on empty space is a click on empty space. Carrying it
        // forward would fire whatever happens to be drawn there next frame.
        let mut input = at(10.0, 10.0);
        input.mouse(MouseButton::Left, true);
        input.scrolled(0.0, -50.0);
        input.key(KeyInput::Enter);

        input.end_frame();

        assert!(!input.take_click(Rect::new(0, 0, 20, 20)));
        assert_eq!(input.take_scroll(Rect::new(0, 0, 20, 20)), (0.0, 0.0));
        assert!(input.keys().is_empty());
    }

    #[test]
    fn holding_the_button_persists_across_frames_but_the_click_does_not() {
        // Held is state; the press is an edge. A widget that wants to look
        // pressed asks the first, a widget that wants to act asks the second.
        let mut input = at(10.0, 10.0);
        input.mouse(MouseButton::Left, true);
        let area = Rect::new(0, 0, 20, 20);

        assert!(input.pressing(area));
        input.end_frame();
        assert!(input.pressing(area), "still held");
        assert!(!input.take_click(area), "but not clicked again");

        input.mouse(MouseButton::Left, false);
        assert!(!input.pressing(area));
    }

    #[test]
    fn a_drag_that_leaves_the_window_does_not_stay_held() {
        // No release arrives when the cursor leaves, so a widget would think
        // the button was down forever.
        let mut input = at(10.0, 10.0);
        input.mouse(MouseButton::Left, true);
        input.cursor_left();

        assert!(!input.pressing(Rect::new(0, 0, 20, 20)));
        assert!(!input.hovering(Rect::new(0, 0, 20, 20)));
        assert_eq!(input.cursor(), None);
    }

    #[test]
    fn scrolling_moves_the_region_under_the_cursor_and_only_that_one() {
        // Nested scrollable regions: the inner one claims the wheel, and the
        // outer one must not also move.
        let mut input = at(10.0, 10.0);
        input.scrolled(0.0, -48.0);

        let outer = Rect::new(0, 0, 100, 100);
        let inner = Rect::new(0, 0, 20, 20);

        assert_eq!(input.take_scroll(inner), (0.0, -48.0));
        assert_eq!(input.take_scroll(outer), (0.0, 0.0));
    }

    #[test]
    fn scrolling_somewhere_else_leaves_it_for_whoever_is_under_the_cursor() {
        let mut input = at(90.0, 90.0);
        input.scrolled(0.0, -48.0);
        assert_eq!(input.take_scroll(Rect::new(0, 0, 20, 20)), (0.0, 0.0));
        assert_eq!(input.take_scroll(Rect::new(80, 80, 40, 40)), (0.0, -48.0));
    }

    #[test]
    fn wheel_movement_within_a_frame_accumulates() {
        // A fast flick arrives as several events before anything redraws.
        let mut input = at(10.0, 10.0);
        input.scrolled(0.0, -10.0);
        input.scrolled(0.0, -15.0);
        assert_eq!(input.take_scroll(Rect::new(0, 0, 20, 20)), (0.0, -25.0));
    }

    #[test]
    fn keys_arrive_in_the_order_they_were_typed() {
        // A cashier types faster than the screen repaints, and an order is
        // not an order if the digits arrive shuffled.
        let mut input = Input::new();
        for character in "341".chars() {
            input.key(KeyInput::Character(character));
        }
        input.key(KeyInput::Enter);

        assert_eq!(
            input.keys(),
            &[
                KeyInput::Character('3'),
                KeyInput::Character('4'),
                KeyInput::Character('1'),
                KeyInput::Enter,
            ]
        );
        assert_eq!(input.take_keys().len(), 4);
        assert!(input.keys().is_empty(), "taken means taken");
    }

    #[test]
    fn a_focused_widget_can_consume_keys_without_leaving_any_for_another() {
        let mut input = Input::new();
        input.key(KeyInput::Character('4'));
        input.key(KeyInput::Enter);
        let seen = input.consume_keys(|keys| keys.to_vec());
        assert_eq!(
            seen,
            vec![KeyInput::Character('4'), KeyInput::Enter],
            "the widget sees the original order"
        );
        assert!(input.keys().is_empty(), "the keys were claimed");
    }

    #[test]
    fn a_right_click_is_separate_from_a_left_one() {
        let mut input = at(10.0, 10.0);
        let area = Rect::new(0, 0, 20, 20);
        input.mouse(MouseButton::Right, true);

        assert!(!input.take_click(area), "not a left click");
        assert!(input.take_context_click(area));
        assert!(!input.take_context_click(area));
    }

    #[test]
    fn a_press_lands_where_the_cursor_was_not_where_it_ends_up() {
        // The button goes down, then the cursor moves before the frame is
        // drawn. The click belongs to what was under it at press time.
        let mut input = at(10.0, 10.0);
        input.mouse(MouseButton::Left, true);
        input.cursor_moved(90.0, 90.0);

        assert!(input.take_click(Rect::new(0, 0, 20, 20)));
    }

    #[test]
    fn a_press_with_the_cursor_outside_the_window_claims_nothing() {
        let mut input = Input::new();
        input.mouse(MouseButton::Left, true);
        assert!(!input.take_click(Rect::new(0, 0, 1_000, 1_000)));
    }
}
