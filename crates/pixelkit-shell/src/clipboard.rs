//! Plain text with the system clipboard.
//!
//! On macOS this is the general `NSPasteboard` behind safe objc2 wrappers
//! (no `unsafe` in this workspace): copying a phone number out of Messages
//! and pasting it into a field works, and copying out of a field works in
//! every other app. Anywhere else, a process-local fallback answers
//! instead. Reads return `None` — never an error — when there is no text,
//! which is also what a failing pasteboard looks like.

/// Process-local fallback where there is no system pasteboard.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Default)]
struct Fallback {
    text: Option<String>,
}

/// The system clipboard for plain text.
#[derive(Debug, Clone, Default)]
pub struct Clipboard {
    #[cfg(not(target_os = "macos"))]
    fallback: Fallback,
}

impl Clipboard {
    /// An empty clipboard handle. Handles are cheap and stateless; the
    /// pasteboard itself lives with the system.
    pub fn new() -> Clipboard {
        Clipboard::default()
    }

    /// Copy `text` onto the clipboard, replacing whatever was there.
    /// Returns whether the text reached the clipboard.
    pub fn set_text(&mut self, text: impl Into<String>) -> bool {
        let text = text.into();
        #[cfg(target_os = "macos")]
        {
            if macos::set(&text) {
                return true;
            }
            return false;
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.fallback.text = Some(text);
            true
        }
    }

    /// The clipboard's text, or `None` when there is none. Never fails.
    pub fn text(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            return macos::get();
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.fallback.text.clone()
        }
    }

    /// Forget the clipboard's contents; [`Clipboard::text`] reads `None`
    /// afterwards.
    pub fn clear(&mut self) {
        #[cfg(target_os = "macos")]
        macos::clear();
        #[cfg(not(target_os = "macos"))]
        {
            self.fallback.text = None;
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;

    /// Run `f` against the general pasteboard. `generalPasteboard` returns
    /// nil — which objc2 reports as a panic, not an error — in processes
    /// without window-server access (a sandbox, an ssh session, parental
    /// controls), so the nil case is caught and reads as `default`. The
    /// clipboard keeps its contract: reads are `None`, never a crash.
    fn with_board<T: Default>(f: impl FnOnce(&NSPasteboard) -> T) -> T {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let board = NSPasteboard::generalPasteboard();
            f(&board)
        }))
        .unwrap_or_default()
    }

    /// The value of `NSPasteboardTypeString`, spelled out: that symbol is an
    /// extern static, and reading one takes `unsafe`, which this workspace
    /// forbids. The string has been `NSStringPboardType` since NeXT, and the
    /// round-trip test below proves the board answers to it.
    const STRING_TYPE: &str = "NSStringPboardType";

    pub fn get() -> Option<String> {
        with_board(|board| {
            board
                .stringForType(&NSString::from_str(STRING_TYPE))
                .map(|value| value.to_string())
        })
    }

    pub fn set(text: &str) -> bool {
        with_board(|board| {
            board.clearContents();
            let value = NSString::from_str(text);
            board.setString_forType(&value, &NSString::from_str(STRING_TYPE))
        })
    }

    pub fn clear() {
        with_board(|board| {
            board.clearContents();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests share one system pasteboard, so they take turns.
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn a_new_clipboard_reads_empty_or_foreign_text_without_failing() {
        let _guard = LOCK.lock().unwrap();
        let mut clipboard = Clipboard::new();
        clipboard.clear();
        assert_eq!(clipboard.text(), None);
    }

    #[test]
    fn copied_text_reads_back() {
        let _guard = LOCK.lock().unwrap();
        let mut clipboard = Clipboard::new();
        assert!(clipboard.set_text("ข้าวผัด 2"));
        assert_eq!(clipboard.text(), Some("ข้าวผัด 2".to_owned()));
    }

    #[test]
    fn copying_replaces_what_was_there() {
        let _guard = LOCK.lock().unwrap();
        let mut clipboard = Clipboard::new();
        assert!(clipboard.set_text("first"));
        assert!(clipboard.set_text("second"));
        assert_eq!(clipboard.text(), Some("second".to_owned()));
    }

    #[test]
    fn clearing_reads_empty_again() {
        let _guard = LOCK.lock().unwrap();
        let mut clipboard = Clipboard::new();
        assert!(clipboard.set_text("something"));
        clipboard.clear();
        assert_eq!(clipboard.text(), None);
    }
}
