//! A winit host: owns the window and the pixel buffer, forwards input,
//! tracks the scale factor, paces redraws, and presents through a
//! [`Presenter`].
//!
//! One difference from a general-purpose shell, and it matters: **Escape is
//! delivered to the application, not treated as quit.** On a keypad, Escape is
//! a working key — it abandons what is being typed. A host that closed the
//! window on it would throw away a half-entered order at a busy counter.
//!
//! The counter client is keyboard-only by design — a cashier's hands belong on
//! the keypad — but the back office is a different job done sitting down, with
//! tables to scroll and rows to pick. So the host also reports the cursor,
//! the buttons, the wheel and the modifier keys. Every one of those arrives
//! through a defaulted trait method, which is what keeps the counter client
//! from having to know they exist.

use std::sync::Arc;
use std::time::{Duration, Instant};

use pixelkit_raster::WindowBuffer;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton as WinitButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::frame_log::FrameTimer;
use crate::present::{Presenter, SoftbufferPresenter};
use crate::scale::Scale;

/// A handle that wakes the window from another thread.
///
/// The reason this exists is latency. Without it a client that receives its
/// state over a socket has only one way to notice something arrived: ask on a
/// timer. A 33 ms timer costs an average of 16 ms between the daemon answering
/// a keystroke and the screen showing it — half a keystroke's worth of delay
/// that no amount of faster drawing recovers — and it costs thirty wakeups a
/// second forever, on a counter where nothing is happening.
///
/// Waking on arrival removes both at once: the redraw happens when there is
/// something new to draw, and an idle terminal sleeps.
///
/// Cloneable and `Send`, so the socket thread keeps one. Waking a window that
/// has already closed is a no-op rather than an error — a snapshot arriving
/// during shutdown is ordinary, not exceptional.
#[derive(Debug, Clone)]
pub struct Waker {
    proxy: Option<EventLoopProxy<Wake>>,
}

/// The only user event: "look again". Deliberately carries nothing — the
/// application already owns its channels, and a payload here would be a second
/// path for state to arrive on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake;

impl Waker {
    /// A waker attached to nothing, for tests and for the `screens` examples
    /// that render without an event loop.
    pub fn detached() -> Waker {
        Waker { proxy: None }
    }

    /// Ask the window to run a tick and repaint as soon as it can.
    pub fn wake(&self) {
        if let Some(proxy) = &self.proxy {
            let _ = proxy.send_event(Wake);
        }
    }
}

/// A wheel notch, in pixels. Platforms that report scrolling in lines rather
/// than pixels get multiplied by this to land somewhere comfortable.
const PIXELS_PER_LINE: f32 = 24.0;

/// A key the host recognised, as the application sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInput {
    /// A printable character, already normalised — numpad and top-row digits
    /// arrive identically, because a cashier should not have to care.
    Character(char),
    Enter,
    Escape,
    Backspace,
    /// A function key, by number.
    Function(u8),
    Left,
    Right,
    Up,
    Down,
    Tab,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Which modifiers were held when an event arrived.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
}

impl Modifiers {
    /// Nothing held. The common case, and worth naming so a caller reads as
    /// "a plain click" rather than "no modifiers".
    pub fn none(self) -> bool {
        !self.shift && !self.control && !self.alt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

pub trait PixelApp {
    /// Paint the whole frame. `buffer` is the window's physical size;
    /// `scale` converts the logical units layout is written in.
    fn render(&mut self, buffer: &mut WindowBuffer, scale: Scale);

    /// The window moved to a display with a different scale factor. The
    /// next `render` receives it too; this is for caches keyed on scale.
    fn on_scale(&mut self, _scale: Scale) {}

    /// Take a handle that wakes this window from another thread.
    ///
    /// Called once, before the loop starts. An application whose state arrives
    /// on a socket should hand this to whatever owns the socket: it is the
    /// difference between the screen updating when the answer arrives and the
    /// screen updating on the next timer.
    fn attach(&mut self, _waker: Waker) {}

    /// How soon the application wants the next unprompted redraw. `None` waits
    /// for input or a wake; a duration ticks.
    ///
    /// Prefer `None` and a [`Waker`]. A tick is for something the application
    /// itself is animating — a flash that has to end on time — not for
    /// noticing that a message arrived, which is what a wake is for. An
    /// interval here is thirty wakeups a second on a machine that is supposed
    /// to be idle between customers.
    fn animation_interval(&self) -> Option<Duration> {
        None
    }

    fn tick(&mut self) {}
    fn on_key(&mut self, _key: &KeyInput) {}
    fn on_exit(&mut self) {}

    /// The modifier state changed. Delivered separately from the keys it
    /// modifies, because shift-clicking is a mouse event that needs to know.
    fn on_modifiers(&mut self, _modifiers: Modifiers) {}

    /// The cursor moved to a position in the buffer's pixel coordinates.
    fn on_cursor(&mut self, _x: f32, _y: f32) {}

    /// The cursor left the window, so nothing is hovered any more. Without
    /// this a highlight stays lit under a cursor that has gone.
    fn on_cursor_left(&mut self) {}

    fn on_mouse(&mut self, _button: MouseButton, _pressed: bool) {}

    /// Wheel or trackpad movement, in pixels. Positive `y` scrolls content
    /// down — the direction the wheel turns, not the direction the view moves.
    fn on_scroll(&mut self, _dx: f32, _dy: f32) {}

    /// Whether the application wants the window closed. Checked after every
    /// tick, so an application can quit itself.
    fn should_exit(&self) -> bool {
        false
    }
}

/// How the window is opened. Sizes are logical pixels.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub min_width: Option<f64>,
    pub min_height: Option<f64>,
    pub resizable: bool,
}

impl WindowConfig {
    pub fn new(title: &str, width: f64, height: f64) -> WindowConfig {
        WindowConfig {
            title: title.to_owned(),
            width,
            height,
            min_width: None,
            min_height: None,
            resizable: true,
        }
    }

    pub fn min_size(mut self, width: f64, height: f64) -> WindowConfig {
        self.min_width = Some(width);
        self.min_height = Some(height);
        self
    }
}

struct Shell<A: PixelApp> {
    app: A,
    config: WindowConfig,
    window: Option<Arc<Window>>,
    presenter: Option<Box<dyn Presenter>>,
    buffer: WindowBuffer,
    scale: Scale,
    timer: FrameTimer,
    /// When the next unprompted repaint is due, for an app that asked for an
    /// interval. `None` until the first one is scheduled.
    next_tick: Option<Instant>,
}

impl<A: PixelApp> Shell<A> {
    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn redraw(&mut self) {
        let Some(window) = &self.window else { return };
        let Some(presenter) = &mut self.presenter else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.buffer.width != size.width || self.buffer.height != size.height {
            self.buffer.resize(size.width, size.height);
            if presenter.resize(size.width, size.height).is_err() {
                return;
            }
        }
        let started = Instant::now();
        self.app.render(&mut self.buffer, self.scale);
        let painted = Instant::now();
        let _ = presenter.present(&self.buffer);
        let presented = Instant::now();
        self.timer
            .record(Duration::ZERO, painted - started, presented - painted);
    }
}

/// Translate a winit key into something a counter understands.
fn translate(logical: &Key, text: Option<&str>) -> Option<KeyInput> {
    match logical {
        Key::Named(NamedKey::Enter) => Some(KeyInput::Enter),
        Key::Named(NamedKey::Escape) => Some(KeyInput::Escape),
        Key::Named(NamedKey::Backspace) => Some(KeyInput::Backspace),
        Key::Named(NamedKey::Tab) => Some(KeyInput::Tab),
        Key::Named(NamedKey::Delete) => Some(KeyInput::Delete),
        Key::Named(NamedKey::Home) => Some(KeyInput::Home),
        Key::Named(NamedKey::End) => Some(KeyInput::End),
        Key::Named(NamedKey::PageUp) => Some(KeyInput::PageUp),
        Key::Named(NamedKey::PageDown) => Some(KeyInput::PageDown),
        Key::Named(NamedKey::ArrowLeft) => Some(KeyInput::Left),
        Key::Named(NamedKey::ArrowRight) => Some(KeyInput::Right),
        Key::Named(NamedKey::ArrowUp) => Some(KeyInput::Up),
        Key::Named(NamedKey::ArrowDown) => Some(KeyInput::Down),
        Key::Named(NamedKey::F1) => Some(KeyInput::Function(1)),
        Key::Named(NamedKey::F2) => Some(KeyInput::Function(2)),
        Key::Named(NamedKey::F3) => Some(KeyInput::Function(3)),
        Key::Named(NamedKey::F4) => Some(KeyInput::Function(4)),
        Key::Named(NamedKey::F5) => Some(KeyInput::Function(5)),
        Key::Named(NamedKey::F12) => Some(KeyInput::Function(12)),
        Key::Character(characters) => characters.chars().next().map(KeyInput::Character),
        // The numpad reports its keys as text rather than as named keys on
        // some platforms; taking the text is what makes a numpad and the top
        // row indistinguishable to the application.
        _ => text
            .and_then(|text| text.chars().next())
            .map(KeyInput::Character),
    }
}

/// Whether an interval-driven repaint is due, and when the next one is.
///
/// Split out from the event loop because getting it wrong does not look like a
/// bug — it looks like a working application. Requesting a redraw on every
/// pass through `about_to_wait` does not tick, it *spins*: a pending redraw is
/// work, so `WaitUntil` never blocks, and the loop dispatches the redraw,
/// comes straight back, asks for another, and repaints as fast as the
/// compositor allows. On a desktop that is an idle-looking client quietly
/// burning a core. On a Raspberry Pi it is the machine, permanently,
/// repainting a screen nobody is looking at.
///
/// Returns `(redraw_now, next_deadline)`.
fn schedule(now: Instant, next_tick: Option<Instant>, interval: Duration) -> (bool, Instant) {
    match next_tick {
        // Due, or overdue because a slow frame or a suspended laptop took us
        // past it. Either way the next deadline is measured from now rather
        // than from the missed one, so a backlog cannot accumulate into a
        // burst of catch-up frames.
        Some(due) if now < due => (false, due),
        _ => (true, now + interval),
    }
}

fn translate_button(button: WinitButton) -> Option<MouseButton> {
    match button {
        WinitButton::Left => Some(MouseButton::Left),
        WinitButton::Right => Some(MouseButton::Right),
        WinitButton::Middle => Some(MouseButton::Middle),
        // Back, forward and the extra buttons on a gaming mouse. Ignored
        // rather than guessed at, for the same reason an unknown key is.
        _ => None,
    }
}

/// Wheel movement in pixels, whichever way the platform reports it.
fn scroll_pixels(delta: MouseScrollDelta) -> (f32, f32) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (x * PIXELS_PER_LINE, y * PIXELS_PER_LINE),
        MouseScrollDelta::PixelDelta(position) => (position.x as f32, position.y as f32),
    }
}

impl<A: PixelApp> ApplicationHandler<Wake> for Shell<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attributes = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_resizable(self.config.resizable)
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height));
        if let (Some(w), Some(h)) = (self.config.min_width, self.config.min_height) {
            attributes = attributes.with_min_inner_size(LogicalSize::new(w, h));
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("a window should be creatable"),
        );
        let presenter = SoftbufferPresenter::new(window.clone()).expect("a software presenter");
        self.scale = Scale::new(window.scale_factor());
        self.app.on_scale(self.scale);
        self.window = Some(window);
        self.presenter = Some(Box::new(presenter));
    }

    /// Something arrived on a thread that is not this one.
    ///
    /// Ticking here rather than leaving it to `about_to_wait` is deliberate:
    /// the drain has to happen before the paint, and this is the one place
    /// that ordering is guaranteed regardless of how the platform schedules
    /// the two. A tick is a channel drain, so running it twice costs nothing.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _wake: Wake) {
        self.app.tick();
        self.request_redraw();
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.timer.flush();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.app.on_exit();
                event_loop.exit();
            }
            WindowEvent::Resized(_) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = Scale::new(scale_factor);
                self.app.on_scale(self.scale);
                // winit follows this with a `Resized` carrying the new
                // physical size, which triggers the repaint.
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(key) = translate(&event.logical_key, event.text.as_deref()) {
                    self.app.on_key(&key);
                    self.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.app.on_modifiers(Modifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                });
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.app.on_cursor(position.x as f32, position.y as f32);
                // A redraw on every cursor move is what makes hover feedback
                // work at all under `ControlFlow::Wait`. It costs a full
                // repaint, which for a screen of panels and text is cheap
                // enough that measuring it was not worth the complexity of
                // tracking which region changed.
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.app.on_cursor_left();
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = translate_button(button) {
                    self.app.on_mouse(button, state == ElementState::Pressed);
                    self.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = scroll_pixels(delta);
                self.app.on_scroll(dx, dy);
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.tick();
        if self.app.should_exit() {
            self.app.on_exit();
            event_loop.exit();
            return;
        }
        match self.app.animation_interval() {
            Some(interval) if interval.is_zero() => {
                event_loop.set_control_flow(ControlFlow::Poll);
                self.request_redraw();
            }
            Some(interval) => {
                let (redraw, deadline) = schedule(Instant::now(), self.next_tick, interval);
                self.next_tick = Some(deadline);
                if redraw {
                    self.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
            None => {
                self.next_tick = None;
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
    }
}

pub fn run_app<A: PixelApp>(
    app: A,
    config: WindowConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    let mut app = app;
    app.attach(Waker {
        proxy: Some(event_loop.create_proxy()),
    });
    let mut shell = Shell {
        app,
        config,
        window: None,
        presenter: None,
        buffer: WindowBuffer::new(1, 1),
        scale: Scale::ONE,
        timer: FrameTimer::from_env(),
        next_tick: None,
    };
    event_loop.run_app(&mut shell)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    #[test]
    fn escape_reaches_the_application_rather_than_closing_the_window() {
        // The whole reason this shell is not the general-purpose one.
        assert_eq!(
            translate(&Key::Named(NamedKey::Escape), None),
            Some(KeyInput::Escape)
        );
    }

    #[test]
    fn digits_arrive_the_same_from_the_numpad_and_the_top_row() {
        // Top row: reported as a character.
        assert_eq!(
            translate(&Key::Character(SmolStr::new("7")), Some("7")),
            Some(KeyInput::Character('7'))
        );
        // Numpad on platforms that report it as an unnamed key with text.
        assert_eq!(
            translate(&Key::Named(NamedKey::Space), Some("7")),
            Some(KeyInput::Character('7'))
        );
    }

    #[test]
    fn the_keypad_operators_all_translate() {
        for symbol in ['+', '-', '*', '/', '.'] {
            assert_eq!(
                translate(&Key::Character(SmolStr::new(symbol.to_string())), None),
                Some(KeyInput::Character(symbol)),
                "{symbol} should reach the application"
            );
        }
    }

    #[test]
    fn function_keys_are_distinguished_from_characters() {
        assert_eq!(
            translate(&Key::Named(NamedKey::F1), None),
            Some(KeyInput::Function(1))
        );
        assert_ne!(
            translate(&Key::Named(NamedKey::F1), None),
            translate(&Key::Named(NamedKey::F2), None)
        );
    }

    #[test]
    fn a_key_with_no_meaning_is_ignored_rather_than_guessed_at() {
        assert_eq!(translate(&Key::Named(NamedKey::Alt), None), None);
    }

    #[test]
    fn the_navigation_keys_a_table_needs_all_arrive() {
        for (named, expected) in [
            (NamedKey::Tab, KeyInput::Tab),
            (NamedKey::Delete, KeyInput::Delete),
            (NamedKey::Home, KeyInput::Home),
            (NamedKey::End, KeyInput::End),
            (NamedKey::PageUp, KeyInput::PageUp),
            (NamedKey::PageDown, KeyInput::PageDown),
        ] {
            assert_eq!(translate(&Key::Named(named), None), Some(expected));
        }
    }

    #[test]
    fn tab_is_a_key_rather_than_a_character() {
        // Winit reports Tab with text of "\t". Falling through to the text
        // branch would make it an unprintable character in a search field
        // instead of moving the focus.
        assert_eq!(
            translate(&Key::Named(NamedKey::Tab), Some("\t")),
            Some(KeyInput::Tab)
        );
    }

    #[test]
    fn the_buttons_a_mouse_has_translate_and_the_extra_ones_do_not() {
        assert_eq!(translate_button(WinitButton::Left), Some(MouseButton::Left));
        assert_eq!(
            translate_button(WinitButton::Right),
            Some(MouseButton::Right)
        );
        assert_eq!(
            translate_button(WinitButton::Middle),
            Some(MouseButton::Middle)
        );
        assert_eq!(translate_button(WinitButton::Back), None);
        assert_eq!(translate_button(WinitButton::Other(9)), None);
    }

    #[test]
    fn scrolling_is_pixels_however_the_platform_reports_it() {
        // X11 and Wayland report lines; a trackpad reports pixels. A widget
        // that had to know which would scroll at two different speeds.
        let (_, lines) = scroll_pixels(MouseScrollDelta::LineDelta(0.0, -2.0));
        assert_eq!(lines, -2.0 * PIXELS_PER_LINE);

        let (dx, dy) = scroll_pixels(MouseScrollDelta::PixelDelta(
            winit::dpi::PhysicalPosition::new(3.0, -7.5),
        ));
        assert_eq!((dx, dy), (3.0, -7.5));

        // Both directions survive, so a wide table can scroll sideways.
        let (dx, _) = scroll_pixels(MouseScrollDelta::LineDelta(1.0, 0.0));
        assert_eq!(dx, PIXELS_PER_LINE);
    }

    #[test]
    fn an_interval_ticks_rather_than_spinning() {
        // The regression this exists for: the first pass repaints, and every
        // pass before the deadline must not. A version that returned `true`
        // here repainted flat out while claiming a 33 ms tick.
        let interval = Duration::from_millis(33);
        let start = Instant::now();

        let (redraw, deadline) = schedule(start, None, interval);
        assert!(redraw, "the first pass paints");
        assert_eq!(deadline, start + interval);

        // Every pass in between — and there are thousands, because input and
        // the redraw itself both bring us back here.
        for micros in [1, 100, 16_000, 32_999] {
            let now = start + Duration::from_micros(micros);
            let (redraw, unchanged) = schedule(now, Some(deadline), interval);
            assert!(!redraw, "repainted {micros}µs into a 33ms interval");
            assert_eq!(unchanged, deadline, "the deadline must not drift");
        }

        // And at the deadline it paints again.
        let (redraw, next) = schedule(deadline, Some(deadline), interval);
        assert!(redraw);
        assert_eq!(next, deadline + interval);
    }

    #[test]
    fn a_missed_deadline_does_not_become_a_burst_of_catch_up_frames() {
        // A suspended laptop or a slow frame can leave us far past due.
        // Measuring the next deadline from the missed one would queue every
        // frame that should have happened while nothing was watching.
        let interval = Duration::from_millis(33);
        let start = Instant::now();
        let missed = start + interval;
        let late = start + Duration::from_secs(60);

        let (redraw, next) = schedule(late, Some(missed), interval);
        assert!(redraw, "one frame, to catch up");
        assert_eq!(
            next,
            late + interval,
            "measured from now, not from the miss"
        );
    }

    #[test]
    fn a_plain_click_is_distinguishable_from_a_modified_one() {
        assert!(Modifiers::default().none());
        assert!(!Modifiers {
            shift: true,
            ..Modifiers::default()
        }
        .none());
    }
}
