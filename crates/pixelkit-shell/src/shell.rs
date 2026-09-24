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
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Ime as WinitIme, MouseButton as WinitButton, MouseScrollDelta, TouchPhase,
    WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey, PhysicalKey};
use winit::window::{Theme as WinitTheme, Window, WindowId};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesExtMacOS;

use crate::frame_log::FrameTimer;
use crate::present::{PresentError, Presenter, SoftbufferPresenter};
use crate::scale::Scale;

/// What arrives on the event loop's user channel: a wake, and, with the
/// `accessibility` feature, a request from assistive technology.
#[derive(Debug)]
pub enum Host {
    Wake,
    #[cfg(feature = "accessibility")]
    Accessibility(accesskit_winit::Event),
}

#[cfg(feature = "accessibility")]
impl From<accesskit_winit::Event> for Host {
    fn from(event: accesskit_winit::Event) -> Host {
        Host::Accessibility(event)
    }
}

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
    proxy: Option<EventLoopProxy<Host>>,
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
            let _ = proxy.send_event(Host::Wake);
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

/// One key transition, including release, the whole committed string, and the
/// physical key. [`PixelApp::on_key`] still receives presses only, as the
/// first character, so existing callers keep working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: KeyInput,
    /// The whole string the key committed. Empty for navigation keys and for
    /// key-up. More than one scalar when the platform delivers one.
    pub text: String,
    /// Debug name of the physical key (`KeyA`, `F13`, …).
    pub physical: String,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: Modifiers,
}

/// Touch or gesture phase. Momentum is [`GesturePhase::Moved`] after
/// [`GesturePhase::Ended`] is not invented here; the platform's phase is forwarded as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GesturePhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

/// A scroll. Line deltas stay distinct from the pixel deltas derived from them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollEvent {
    pub pixel_dx: f32,
    pub pixel_dy: f32,
    /// Set when the platform reported lines rather than pixels.
    pub line_dx: Option<f32>,
    pub line_dy: Option<f32>,
    pub phase: GesturePhase,
}

/// Where the IME candidate window should sit, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImeCursorArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Which modifiers were held when an event arrived.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// Cmd on macOS, the Windows/Super key elsewhere. Named `logo` after
    /// winit's own `super_key()`, since "super" reads as a privilege level
    /// to most of this codebase's readers.
    pub logo: bool,
}

impl Modifiers {
    /// Nothing held. The common case, and worth naming so a caller reads as
    /// "a plain click" rather than "no modifiers".
    pub fn none(self) -> bool {
        !self.shift && !self.control && !self.alt && !self.logo
    }

    /// The platform's accelerator modifier for a keyboard shortcut: Cmd on
    /// macOS, Ctrl everywhere else. A screen wiring up ⌘N/Ctrl+N checks this
    /// instead of `control` so the shortcut is right on both platforms.
    pub fn accel(self) -> bool {
        if cfg!(target_os = "macos") {
            self.logo
        } else {
            self.control
        }
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

    /// Press and release, with the full committed string and the physical key.
    fn on_key_event(&mut self, _event: &KeyEvent) {}

    /// IME composition: CJK, emoji-picker, dictation. `Commit` text belongs
    /// in the focused field (see `TextFieldState::insert_str`); `Preedit` is
    /// for showing the in-progress composition. Direct-key typing — Thai
    /// stacks included — still arrives through `on_key`, unchanged.
    fn on_ime(&mut self, _ime: &ImeEvent) {}

    /// A new window title to apply, or `None` for no change. The shell drains
    /// this every frame and calls `Window::set_title`, so per-section
    /// subtitles need no window access. One-shot by convention: return the
    /// title once, then `None`.
    fn poll_title(&mut self) -> Option<String> {
        None
    }

    /// A change of full-screen state the application wants. Polled like
    /// [`PixelApp::poll_title`]: return `Some(true)` once to enter borderless
    /// full screen on the current monitor, `Some(false)` once to leave it.
    fn poll_fullscreen(&mut self) -> Option<bool> {
        None
    }

    /// The pointer shape the application wants over its window now. Polled
    /// after input; the shell only touches the window when it changes.
    fn cursor_shape(&self) -> CursorShape {
        CursorShape::Default
    }

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

    /// Scroll with its phase and, when the platform sent lines, those lines
    /// kept separate from the pixel conversion.
    fn on_scroll_event(&mut self, _event: &ScrollEvent) {}

    /// Pinch / magnify. `delta` is the platform's magnification step.
    fn on_magnify(&mut self, _phase: GesturePhase, _delta: f64) {}

    /// Caret rectangle for the IME candidate window, in physical pixels.
    /// `None` leaves the platform's last rectangle alone.
    fn ime_cursor_area(&self) -> Option<ImeCursorArea> {
        None
    }

    /// Whether the application wants the window closed. Checked after every
    /// tick, so an application can quit itself.
    fn should_exit(&self) -> bool {
        false
    }

    /// The window's system theme is `dark`. Called once at startup (if the
    /// platform reports one) and again on every change, so a screen can
    /// switch [`crate::palette`]-style light/dark colours without polling.
    fn on_theme(&mut self, _dark: bool) {}

    /// The application's semantic tree for assistive technology, or `None`
    /// for an application that has none.
    ///
    /// Asked for once when a screen reader attaches and again after every
    /// paint while one is attached, so it describes the frame that was just
    /// drawn: the same widgets, with the same bounds in physical pixels.
    /// The toolkit keeps no widget identity — that is the immediate-mode
    /// bargain — so the ids in the tree are the application's own, chosen
    /// from what a control means rather than where it sits.
    #[cfg(feature = "accessibility")]
    fn accessibility_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        None
    }

    /// Assistive technology asked for an action on a node the application
    /// published, such as a click on a button.
    #[cfg(feature = "accessibility")]
    fn on_accessibility_action(&mut self, _request: accesskit::ActionRequest) {}
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
    /// macOS: draw the titlebar transparent, so content can show through it.
    /// Ignored on every other platform.
    pub titlebar_transparent: bool,
    /// macOS: hide the title text in the titlebar. Ignored elsewhere.
    pub title_hidden: bool,
    /// macOS: let content extend under the titlebar's full-size content
    /// view, the usual companion to `titlebar_transparent`. Ignored
    /// elsewhere.
    pub fullsize_content_view: bool,
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
            titlebar_transparent: false,
            title_hidden: false,
            fullsize_content_view: false,
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
    /// Duration of the most recent `tick`, recorded into the next present.
    pending_tick: Duration,
    /// First input since the last present. Cleared when that present is timed.
    input_at: Option<Instant>,
    modifiers: Modifiers,
    /// `None` selects the softbuffer presenter. Taken once in `resumed`.
    presenter_factory: Option<PresenterFactory>,
    /// When the next unprompted repaint is due, for an app that asked for an
    /// interval. `None` until the first one is scheduled.
    next_tick: Option<Instant>,
    /// The pointer shape last set on the window.
    cursor_shape: CursorShape,
    #[cfg(feature = "accessibility")]
    proxy: EventLoopProxy<Host>,
    #[cfg(feature = "accessibility")]
    accessibility: Option<accesskit_winit::Adapter>,
    /// Whether the application has published a tree, so updates are pushed
    /// only to a tree that exists.
    #[cfg(feature = "accessibility")]
    tree_published: bool,
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
        if let Some(area) = self.app.ime_cursor_area() {
            apply_ime(window, Some(area));
        }
        let started = Instant::now();
        self.app.render(&mut self.buffer, self.scale);
        let painted = Instant::now();
        let _ = presenter.present(&self.buffer);
        let presented = Instant::now();
        let input_to_present = self
            .input_at
            .take()
            .map(|at| presented.saturating_duration_since(at));
        let tick = self.pending_tick;
        self.pending_tick = Duration::ZERO;
        self.timer.record_full(
            tick,
            painted - started,
            presented - painted,
            input_to_present,
        );
        // After the present, so a screen reader is never told about a frame
        // the eyes cannot see yet.
        self.publish_accessibility();
    }
}

impl<A: PixelApp> Shell<A> {
    /// Push the frame just drawn to assistive technology, if any is listening
    /// and the application has a tree to give.
    #[cfg(feature = "accessibility")]
    fn publish_accessibility(&mut self) {
        if !self.tree_published {
            return;
        }
        let Some(adapter) = &mut self.accessibility else {
            return;
        };
        let app = &mut self.app;
        let mut tree = None;
        adapter.update_if_active(|| {
            tree = app.accessibility_tree();
            tree.take().unwrap_or_else(|| accesskit::TreeUpdate {
                nodes: Vec::new(),
                tree: None,
                tree_id: accesskit::TreeId::ROOT,
                focus: accesskit::NodeId(0),
            })
        });
    }

    #[cfg(not(feature = "accessibility"))]
    fn publish_accessibility(&mut self) {}
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
        Key::Named(named) => {
            if let Some(number) = function_key(*named) {
                Some(KeyInput::Function(number))
            } else {
                text.and_then(|text| text.chars().next())
                    .map(KeyInput::Character)
            }
        }
        Key::Character(characters) => characters.chars().next().map(KeyInput::Character),
        // The numpad reports its keys as text rather than as named keys on
        // some platforms; taking the text is what makes a numpad and the top
        // row indistinguishable to the application.
        _ => text
            .and_then(|text| text.chars().next())
            .map(KeyInput::Character),
    }
}

/// IME composition, as the application sees it: a dependency-free mirror of
/// `winit::event::Ime`, so tests never need a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    /// The input method attached; expect `Preedit`/`Commit` to follow.
    Enabled,
    /// In-progress composition and an optional byte-indexed cursor range.
    /// An empty string clears the composition.
    Preedit(String, Option<(usize, usize)>),
    /// Finished text to insert into the focused field.
    Commit(String),
    /// The input method detached; drop any pending composition.
    Disabled,
}

fn function_key(named: NamedKey) -> Option<u8> {
    Some(match named {
        NamedKey::F1 => 1,
        NamedKey::F2 => 2,
        NamedKey::F3 => 3,
        NamedKey::F4 => 4,
        NamedKey::F5 => 5,
        NamedKey::F6 => 6,
        NamedKey::F7 => 7,
        NamedKey::F8 => 8,
        NamedKey::F9 => 9,
        NamedKey::F10 => 10,
        NamedKey::F11 => 11,
        NamedKey::F12 => 12,
        NamedKey::F13 => 13,
        NamedKey::F14 => 14,
        NamedKey::F15 => 15,
        NamedKey::F16 => 16,
        NamedKey::F17 => 17,
        NamedKey::F18 => 18,
        NamedKey::F19 => 19,
        NamedKey::F20 => 20,
        NamedKey::F21 => 21,
        NamedKey::F22 => 22,
        NamedKey::F23 => 23,
        NamedKey::F24 => 24,
        _ => return None,
    })
}

/// The whole string a key committed. Navigation keys contribute nothing, so
/// Tab stays a key instead of the `"\t"` winit attaches to it.
pub fn committed_text(logical: &Key, text: Option<&str>) -> String {
    match logical {
        Key::Character(characters) => {
            if let Some(text) = text.filter(|text| !text.is_empty()) {
                text.to_string()
            } else {
                characters.to_string()
            }
        }
        _ => String::new(),
    }
}

pub fn physical_label(key: PhysicalKey) -> String {
    match key {
        PhysicalKey::Code(code) => format!("{code:?}"),
        PhysicalKey::Unidentified(native) => format!("unidentified:{native:?}"),
    }
}

pub fn interpret_key(
    logical: &Key,
    text: Option<&str>,
    physical: &str,
    pressed: bool,
    repeat: bool,
    modifiers: Modifiers,
) -> Option<KeyEvent> {
    let key = translate(logical, text)?;
    Some(KeyEvent {
        key,
        text: if pressed {
            committed_text(logical, text)
        } else {
            String::new()
        },
        physical: physical.to_string(),
        pressed,
        repeat,
        modifiers,
    })
}

pub fn gesture_phase(phase: TouchPhase) -> GesturePhase {
    match phase {
        TouchPhase::Started => GesturePhase::Started,
        TouchPhase::Moved => GesturePhase::Moved,
        TouchPhase::Ended => GesturePhase::Ended,
        TouchPhase::Cancelled => GesturePhase::Cancelled,
    }
}

/// Pixel deltas for existing callers, plus the original line deltas when the
/// platform reported lines.
pub fn scroll_event(delta: MouseScrollDelta, phase: TouchPhase) -> ScrollEvent {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => ScrollEvent {
            pixel_dx: x * PIXELS_PER_LINE,
            pixel_dy: y * PIXELS_PER_LINE,
            line_dx: Some(x),
            line_dy: Some(y),
            phase: gesture_phase(phase),
        },
        MouseScrollDelta::PixelDelta(position) => ScrollEvent {
            pixel_dx: position.x as f32,
            pixel_dy: position.y as f32,
            line_dx: None,
            line_dy: None,
            phase: gesture_phase(phase),
        },
    }
}

/// Turn IME on and, when the app has a caret rectangle, park the candidate
/// window on it.
pub fn apply_ime(window: &Window, area: Option<ImeCursorArea>) {
    window.set_ime_allowed(true);
    if let Some(area) = area {
        window.set_ime_cursor_area(
            PhysicalPosition::new(area.x, area.y),
            PhysicalSize::new(area.width.max(1.0), area.height.max(1.0)),
        );
    }
}

fn translate_ime(ime: &WinitIme) -> ImeEvent {
    match ime {
        WinitIme::Enabled => ImeEvent::Enabled,
        WinitIme::Preedit(text, cursor) => ImeEvent::Preedit(text.clone(), *cursor),
        WinitIme::Commit(text) => ImeEvent::Commit(text.clone()),
        WinitIme::Disabled => ImeEvent::Disabled,
    }
}

/// Apply a pending title request, if any. Split out so the one-shot
/// convention is testable without a window: with no window yet, the app is
/// not even asked, so an early title is never lost.
fn drain_title<A: PixelApp>(app: &mut A, window: Option<&Window>) {
    let Some(window) = window else { return };
    if let Some(title) = app.poll_title() {
        window.set_title(&title);
    }
}

/// Apply a full-screen request and a pointer-shape change, if any.
fn drain_window_state<A: PixelApp>(app: &mut A, window: Option<&Window>, shape: &mut CursorShape) {
    let Some(window) = window else { return };
    if let Some(full) = app.poll_fullscreen() {
        window.set_fullscreen(full.then_some(winit::window::Fullscreen::Borderless(None)));
    }
    let wanted = app.cursor_shape();
    if wanted != *shape {
        *shape = wanted;
        window.set_cursor(match wanted {
            CursorShape::Default => winit::window::CursorIcon::Default,
            CursorShape::Text => winit::window::CursorIcon::Text,
            CursorShape::Pointer => winit::window::CursorIcon::Pointer,
            CursorShape::ResizeHorizontal => winit::window::CursorIcon::ColResize,
        });
    }
}

/// Pointer shapes an application can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// The platform arrow.
    #[default]
    Default,
    /// An I-beam over editable text.
    Text,
    /// A hand over something clickable.
    Pointer,
    /// A left-right arrow over a draggable vertical edge.
    ResizeHorizontal,
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

impl<A: PixelApp> ApplicationHandler<Host> for Shell<A> {
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
        #[cfg(target_os = "macos")]
        {
            attributes = attributes
                .with_titlebar_transparent(self.config.titlebar_transparent)
                .with_title_hidden(self.config.title_hidden)
                .with_fullsize_content_view(self.config.fullsize_content_view);
        }
        #[cfg(feature = "accessibility")]
        {
            // The adapter attaches to the view before it is ever shown, so
            // the first thing assistive technology sees is a described window.
            attributes = attributes.with_visible(false);
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("a window should be creatable"),
        );
        #[cfg(feature = "accessibility")]
        {
            self.accessibility = Some(accesskit_winit::Adapter::with_event_loop_proxy(
                event_loop,
                &window,
                self.proxy.clone(),
            ));
            window.set_visible(true);
        }
        apply_ime(&window, self.app.ime_cursor_area());
        let presenter: Box<dyn Presenter> = match self.presenter_factory.take() {
            Some(factory) => factory(window.clone()).expect("the application's presenter"),
            None => {
                Box::new(SoftbufferPresenter::new(window.clone()).expect("a software presenter"))
            }
        };
        self.scale = Scale::new(window.scale_factor());
        self.app.on_scale(self.scale);
        if let Some(theme) = window.theme() {
            self.app.on_theme(theme == WinitTheme::Dark);
        }
        self.window = Some(window);
        self.presenter = Some(presenter);
    }

    /// Something arrived on a thread that is not this one.
    ///
    /// Ticking here rather than leaving it to `about_to_wait` is deliberate:
    /// the drain has to happen before the paint, and this is the one place
    /// that ordering is guaranteed regardless of how the platform schedules
    /// the two. A tick is a channel drain, so running it twice costs nothing.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: Host) {
        match event {
            Host::Wake => {
                self.run_tick();
                self.request_redraw();
            }
            #[cfg(feature = "accessibility")]
            Host::Accessibility(event) => {
                use accesskit_winit::WindowEvent as Access;
                match event.window_event {
                    Access::InitialTreeRequested => {
                        if let Some(tree) = self.app.accessibility_tree() {
                            self.tree_published = true;
                            if let Some(adapter) = &mut self.accessibility {
                                adapter.update_if_active(|| tree);
                            }
                        }
                    }
                    Access::ActionRequested(request) => {
                        self.app.on_accessibility_action(request);
                        self.request_redraw();
                    }
                    Access::AccessibilityDeactivated => {}
                }
            }
        }
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
        #[cfg(feature = "accessibility")]
        if let (Some(adapter), Some(window)) = (&mut self.accessibility, &self.window) {
            adapter.process_event(window, &event);
        }
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
                let physical = physical_label(event.physical_key);
                if let Some(decoded) = interpret_key(
                    &event.logical_key,
                    event.text.as_deref(),
                    &physical,
                    event.state == ElementState::Pressed,
                    event.repeat,
                    self.modifiers,
                ) {
                    if decoded.pressed {
                        self.app.on_key(&decoded.key);
                    }
                    self.app.on_key_event(&decoded);
                    self.note_input();
                    self.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = Modifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    logo: state.super_key(),
                };
                self.app.on_modifiers(self.modifiers);
            }
            WindowEvent::ThemeChanged(theme) => {
                self.app.on_theme(theme == WinitTheme::Dark);
                self.request_redraw();
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
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let scrolled = scroll_event(delta, phase);
                self.app.on_scroll(scrolled.pixel_dx, scrolled.pixel_dy);
                self.app.on_scroll_event(&scrolled);
                self.note_input();
                self.request_redraw();
            }
            WindowEvent::PinchGesture { delta, phase, .. } => {
                self.app.on_magnify(gesture_phase(phase), delta);
                self.note_input();
                self.request_redraw();
            }
            WindowEvent::Ime(ime) => {
                self.app.on_ime(&translate_ime(&ime));
                self.note_input();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.run_tick();
        drain_title(&mut self.app, self.window.as_deref());
        drain_window_state(
            &mut self.app,
            self.window.as_deref(),
            &mut self.cursor_shape,
        );
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

pub type PresenterFactory =
    Box<dyn FnOnce(Arc<Window>) -> Result<Box<dyn Presenter>, PresentError>>;

impl<A: PixelApp> Shell<A> {
    fn run_tick(&mut self) {
        let started = Instant::now();
        self.app.tick();
        self.pending_tick = started.elapsed();
    }

    fn note_input(&mut self) {
        if self.input_at.is_none() {
            self.input_at = Some(Instant::now());
        }
    }
}

pub fn run_app<A: PixelApp>(
    app: A,
    config: WindowConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_app_with_presenter(app, config, None)
}

/// `presenter` replaces the softbuffer default. The closure receives the
/// window once, when it is created.
pub fn run_app_with_presenter<A: PixelApp>(
    app: A,
    config: WindowConfig,
    presenter: Option<PresenterFactory>,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<Host>::with_user_event().build()?;
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
        pending_tick: Duration::ZERO,
        input_at: None,
        modifiers: Modifiers::default(),
        presenter_factory: presenter,
        next_tick: None,
        cursor_shape: CursorShape::Default,
        #[cfg(feature = "accessibility")]
        proxy: event_loop.create_proxy(),
        #[cfg(feature = "accessibility")]
        accessibility: None,
        #[cfg(feature = "accessibility")]
        tree_published: false,
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
        assert!(!Modifiers {
            logo: true,
            ..Modifiers::default()
        }
        .none());
    }

    #[test]
    fn accel_reads_logo_on_macos_and_control_elsewhere() {
        let ctrl_only = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        let logo_only = Modifiers {
            logo: true,
            ..Modifiers::default()
        };
        if cfg!(target_os = "macos") {
            assert!(logo_only.accel());
            assert!(!ctrl_only.accel());
        } else {
            assert!(ctrl_only.accel());
            assert!(!logo_only.accel());
        }
    }

    struct TitleApp {
        polls: usize,
        pending: Option<String>,
    }

    impl PixelApp for TitleApp {
        fn render(&mut self, _buffer: &mut WindowBuffer, _scale: Scale) {}

        fn poll_title(&mut self) -> Option<String> {
            self.polls += 1;
            self.pending.take()
        }
    }

    struct BareApp;

    impl PixelApp for BareApp {
        fn render(&mut self, _buffer: &mut WindowBuffer, _scale: Scale) {}
    }

    #[test]
    fn hooks_an_app_does_not_override_are_quiet() {
        // New trait methods must not disturb existing apps: the defaults are
        // a no-op IME hook and no title request.
        let mut app = BareApp;
        app.on_ime(&ImeEvent::Commit("x".to_owned()));
        app.on_key_event(&KeyEvent {
            key: KeyInput::Character('a'),
            text: "a".to_owned(),
            physical: "KeyA".to_owned(),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        });
        app.on_scroll_event(&ScrollEvent {
            pixel_dx: 0.0,
            pixel_dy: 1.0,
            line_dx: None,
            line_dy: None,
            phase: GesturePhase::Moved,
        });
        app.on_magnify(GesturePhase::Ended, 0.1);
        assert!(app.ime_cursor_area().is_none());
        assert_eq!(app.poll_title(), None);
    }

    #[test]
    fn ime_events_all_reach_the_app_rather_than_being_dropped() {
        assert_eq!(translate_ime(&WinitIme::Enabled), ImeEvent::Enabled);
        assert_eq!(
            translate_ime(&WinitIme::Preedit("あ".to_owned(), Some((3, 3)))),
            ImeEvent::Preedit("あ".to_owned(), Some((3, 3)))
        );
        assert_eq!(
            translate_ime(&WinitIme::Preedit(String::new(), None)),
            ImeEvent::Preedit(String::new(), None),
            "the empty preedit that clears a composition survives too"
        );
        assert_eq!(
            translate_ime(&WinitIme::Commit("あ不".to_owned())),
            ImeEvent::Commit("あ不".to_owned())
        );
        assert_eq!(translate_ime(&WinitIme::Disabled), ImeEvent::Disabled);
    }

    #[test]
    fn a_title_waits_for_the_window_rather_than_being_lost() {
        // `about_to_wait` runs before the first window exists. Asking the
        // app then would consume a one-shot title nobody could apply.
        let mut app = TitleApp {
            polls: 0,
            pending: Some("Salon \u{2014} Checkout".to_owned()),
        };
        drain_title(&mut app, None);
        assert_eq!(app.polls, 0, "the app must not be asked with no window");
        assert!(app.pending.is_some(), "the title is still there for later");
    }

    #[test]
    fn a_title_is_one_shot() {
        let mut app = TitleApp {
            polls: 0,
            pending: Some("Salon \u{2014} Checkout".to_owned()),
        };
        assert!(app.poll_title().is_some());
        assert_eq!(app.poll_title(), None, "the second drain finds nothing");
    }

    #[test]
    fn a_committed_string_is_delivered_whole_and_key_up_is_separate() {
        let pressed = interpret_key(
            &Key::Character(SmolStr::new("abc")),
            Some("abc"),
            "KeyA",
            true,
            false,
            Modifiers::default(),
        )
        .expect("character");
        assert_eq!(pressed.key, KeyInput::Character('a'));
        assert_eq!(pressed.text, "abc");
        assert!(pressed.pressed);

        let released = interpret_key(
            &Key::Character(SmolStr::new("a")),
            Some("a"),
            "KeyA",
            false,
            false,
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        )
        .expect("release");
        assert!(!released.pressed);
        assert!(released.text.is_empty());
        assert!(released.modifiers.shift);
        assert_eq!(released.physical, "KeyA");
    }

    #[test]
    fn function_keys_cover_the_whole_range() {
        assert_eq!(
            translate(&Key::Named(NamedKey::F13), None),
            Some(KeyInput::Function(13))
        );
        assert_eq!(
            translate(&Key::Named(NamedKey::F24), None),
            Some(KeyInput::Function(24))
        );
        assert_eq!(
            translate(&Key::Named(NamedKey::F6), None),
            Some(KeyInput::Function(6))
        );
    }

    #[test]
    fn scroll_keeps_line_deltas_and_forwards_the_phase() {
        let lines = scroll_event(MouseScrollDelta::LineDelta(1.0, -2.0), TouchPhase::Ended);
        assert_eq!(lines.line_dx, Some(1.0));
        assert_eq!(lines.line_dy, Some(-2.0));
        assert_eq!(lines.pixel_dy, -2.0 * PIXELS_PER_LINE);
        assert_eq!(lines.phase, GesturePhase::Ended);

        let pixels = scroll_event(
            MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(4.0, 5.0)),
            TouchPhase::Moved,
        );
        assert_eq!(pixels.line_dx, None);
        assert_eq!((pixels.pixel_dx, pixels.pixel_dy), (4.0, 5.0));
        assert_eq!(pixels.phase, GesturePhase::Moved);
        assert_eq!(
            gesture_phase(TouchPhase::Cancelled),
            GesturePhase::Cancelled
        );
        assert_eq!(gesture_phase(TouchPhase::Started), GesturePhase::Started);
    }
}
