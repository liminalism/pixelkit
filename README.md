# pixelkit

`pixelkit` is a Rust workspace for software-rendered desktop interfaces. It provides a pixel buffer and rasterizer, a small TrueType and OpenType text stack, a desktop window host, and immediate-mode controls. Applications own the screen state and draw each frame into the pixel buffer.

## Features

- Software rendering into an XRGB `u32` buffer, with clipping, fills, alpha blending, and reusable raster coverage storage.
- Analytic area coverage for anti-aliased paths and glyphs, with non-zero and even-odd fill rules.
- Vector paths for lines, quadratic and cubic curves, circles, ellipses, rings, rounded rectangles, and stroked polylines.
- PNG read and write support, plus bitmap compositing with nearest-neighbour or bilinear resampling. Bilinear sampling interpolates in premultiplied-alpha space to avoid edge fringes.
- Embedded TrueType `glyf` face parsing, glyph outlines and metrics, character coverage checks, fallback face chains, and text styles with size and tracking.
- OpenType shaping for supported GSUB and GPOS lookup types, including contextual substitutions, ligatures, kerning, mark-to-base and mark-to-mark attachment. The implementation includes the lookups needed by the bundled Latin and Thai sample fonts.
- Text measurement, wrapping, truncation, alignment, rasterization, and a bounded string-level cache.
- A `winit` 0.30 desktop host with single- and multi-window runs, `softbuffer` presentation, redraw pacing, HiDPI scale conversion, keyboard and pointer input, scroll and magnify gestures, IME events, clipboard access, and system appearance helpers.
- Tiled in-app panes with draggable dividers and floating in-app windows that move, resize, raise, and close.
- An optional presenter interface for alternate presentation backends, plus opt-in AccessKit accessibility tree and action hooks.
- Immediate-mode widgets for labels, buttons, tabs, segmented controls, steppers, checkboxes, toggles, radio groups, dropdowns, text fields, tables, scroll lists, tooltips, badges, meters, tracks, pills, panels, and statistics.
- Focus and keyboard navigation helpers, virtualized tables and lists, and flow-grid layout helpers.
- Frame timing summaries with p50, p95, and maximum durations for tick, paint, present, and input-to-present phases.
- `unsafe_code = "forbid"` throughout the workspace.

## Workspace crates

| Crate | Purpose | Dependencies |
|---|---|---|
| [`pixelkit-raster`](crates/pixelkit-raster) | Pixel buffers, painter, analytic path coverage, bitmap scaling and PNG decode/encode | None |
| [`pixelkit-text`](crates/pixelkit-text) | TrueType parsing, OpenType shaping, glyph rasterization, wrapping and text cache | `pixelkit-raster` |
| [`pixelkit-shell`](crates/pixelkit-shell) | Per-frame input, scale handling, presentation interfaces and native clipboard | `pixelkit-raster`, `zeroize`, platform clipboard libraries |
| [`pixelkit-ui`](crates/pixelkit-ui) | Immediate-mode widgets, layout and grapheme-safe editing | `pixelkit-raster`, `pixelkit-text`, `pixelkit-shell`, `unicode-segmentation`, `zeroize`; optional AccessKit |
| [`pixelkit-windowing`](crates/pixelkit-windowing) | OS window hosting, draggable panes and floating windows | `winit`, `softbuffer`, `pixelkit-raster`, `pixelkit-shell` |

The raster and text core are independent of the native window host. UI uses `unicode-segmentation` for extended-grapheme editing and `zeroize` for sensitive state; its optional `accessibility` feature collects AccessKit semantics. Shell owns input and native clipboard primitives. `winit` and `softbuffer` belong to windowing, whose optional `accessibility` feature supplies the platform adapter. On macOS, clipboard and platform helpers use safe `objc2` wrappers. Applications provide and embed their own production fonts.

`pixelkit-ui::Theme` font sizes are logical when passed to `Ui::new`; the UI's theme copy converts
body and heading sizes to device pixels once. Dimensional theme tokens still use `Ui::px`.
Explicit `label_styled` styles remain device-pixel styles, matching `pixelkit-text::TextStyle`;
convert a custom logical style with `Ui::text_style` before both measurement and drawing.
Do not rescale `Ui::theme.body` or `Ui::theme.heading`, or reuse that effective theme as a
logical theme when constructing another scaled UI. Keep the application's unscaled theme instead.

## Requirements

- Rust 1.85 or newer (the workspace toolchain is `stable`).
- A desktop environment supported by `winit` and `softbuffer` to run the live window example.

## Build and examples

```sh
# Build all crates
cargo build --workspace

# Run the workspace tests
cargo test --workspace

# Render the UI gallery to target/gallery.png (no window required)
cargo run -p pixelkit-ui --example gallery --features test-fonts

# Render at 2× scale
cargo run -p pixelkit-ui --example gallery --features test-fonts -- 2

# Open the interactive widget demo
cargo run -p pixelkit-ui --example window --features test-fonts

# Draggable panes and floating windows in one window
cargo run -p pixelkit-windowing --example panes

# Two OS windows sharing one event loop
cargo run -p pixelkit-windowing --example multi

# Render a glyph sheet and report missing characters
cargo run -p pixelkit-text --example glyph_sheet --features test-fonts -- [font.ttf ...]
```

`test-fonts` embeds Noto Sans and Noto Sans Thai for examples and tests; it is not enabled by default in application builds. See [`crates/pixelkit-text/test-fonts/README.md`](crates/pixelkit-text/test-fonts/README.md) for font details and licensing.

To log frame timings from an application that uses the shell:

```sh
PIXELKIT_FRAME_LOG=1 cargo run -p your-app
```

The logger periodically reports p50, p95, and maximum timings for the phases it receives.

## Application structure

An application implements `pixelkit_windowing::PixelApp`. The host calls its rendering and event hooks, provides a `WindowBuffer`, and presents completed frames. The application can keep its widget state, `Input`, `TextCache`, and `RasterKernel` as ordinary fields. The UI example in [`crates/pixelkit-ui/examples/window.rs`](crates/pixelkit-ui/examples/window.rs) shows this arrangement.

At a high level, a render pass creates a `Painter` over the supplied buffer and a `Ui` over the painter, text cache, input state, theme, scale, and raster kernel. Draw the desired controls, update application-owned state from their return values, and call `Input::end_frame()` when frame input has been consumed. Coordinates passed to `Ui` are logical pixels and are converted using the supplied `Scale`.

For applications that need a different display path, implement the shell's `Presenter` interface and use `pixelkit_windowing::run_app_with_presenter` (or `run_windows_with_presenter` for several windows). The default paths use the built-in `SoftbufferPresenter`.

## Crate notes

### `pixelkit-raster`

`WindowBuffer` stores one `0x00RRGGBB` pixel per location. `Painter` provides clipped pixel writes, blending, rectangles, rounded rectangles, rules, and path fills. `RasterKernel` computes analytic coverage for paths, which gives vector edges and text the same anti-aliasing approach. `Bitmap` uses `0xAARRGGBB` straight-alpha pixels; `Painter::blit` composites and optionally scales a bitmap.

The PNG decoder handles 8-bit grayscale, RGB, indexed, and RGBA images, including `tRNS` transparency and Adam7 interlacing. It checks chunk CRCs and bounds and limits declared dimensions before allocating. The encoder writes a `WindowBuffer` as PNG.

### `pixelkit-text`

Faces are parsed from application-provided static bytes with `Face::from_bytes` and registered in a `FontSet`. A `TextStyle` selects a face, pixel size, and letter spacing. The shaping and rendering functions can be used directly, or `TextCache` can cache layout and rendered coverage for repeated UI strings.

The implementation targets the TrueType outlines and OpenType substitutions and positioning used by this project. GSUB support includes single, multiple, alternate, ligature, and chained contextual substitutions; GPOS support includes pair positioning and mark attachment. Unsupported lookup types are skipped. This is a focused text stack, not a claim of complete Unicode shaping coverage.

### `pixelkit-shell`

`Input` collects a frame's worth of cursor, button, wheel and key activity. `Scale` converts logical layout to physical pixels. `Presenter` carries a frame to the screen; `FrameTimer` logs timing. Clipboard uses NSPasteboard on macOS and arboard 3.6.1 on Linux (X11 selections or Wayland wlr data-control), retaining ownership for the handle's lifetime. Unavailable transport is a real error, never successful local-only copy. Other unsupported platforms report unavailable. The local fake is explicitly test-only.

For event-loop-compatible clipboard delivery, create `clipboard::ClipboardWorker::new(move || waker.wake())` in `PixelApp::attach` and pass it to `Input::attach_clipboard`. The bounded worker serializes operations and preserves the native owner. `try_set_text`/`try_text`/`try_clear` expose failures; the older boolean/optional convenience methods still use the real transport. The permanent `clipboard_peer` example supports `set TEXT HOLD_MS` and `get` in separate processes on an explicitly chosen private display; it is not a simulated clipboard test.

### `pixelkit-windowing`

`PixelApp` exposes hooks for drawing, ticks, keyboard events, IME input, cursor and mouse events, scrolling, magnification, theme changes, and exit handling. The host translates platform events into these hooks, tracks physical and logical scale, and presents the software-rendered buffer. `Waker` can request a redraw. `run_app` runs one window; `run_windows` runs several, each with its own application, and applications can open or close windows at runtime through `poll_window`.

Printable key text is committed in full, including a platform `NamedKey::Space`
event carrying `" "`. Navigation keys, Tab and Enter remain key actions rather than
inserting their platform control-text payloads into a text field.

`run_window_service(start, incoming)` is the same host with a persistent idle lifetime: construct it once on the process main thread, give the producer its `Waker` in `start`, and build new applications on the main thread in `incoming`. Every request closes only its own window through `WindowRequest::Close`. `on_host_error` separates creation/presentation failure from user close; startup activation tokens are per-window attributes, not process-global environment changes.

`Panes` tiles a region with draggable dividers: every interior edge resizes the panes on both sides. `Windows` manages floating windows that move by their title bars, resize by their edges and corners, raise on press, and report close-button presses. Both are caller-owned state updated once a frame before drawing.

Enable `accessibility` on `pixelkit-windowing` for the platform AccessKit adapter, including AT-SPI on Linux. Enable `pixelkit-ui/accessibility` and opt into `Ui::with_semantics` to collect labels, roles, checked states, physical bounds and focus from the existing controls. `Semantics::begin` assigns never-reused frame IDs, so a delayed action cannot activate a substituted same-position control. `begin_scoped(title, epoch)` can preserve unchanged role/label IDs only when the application guarantees the same logical rows, page and geometry within that epoch; otherwise use the conservative default. Call `invalidate()` before identity/layout changes, including the host's `on_layout_changed` callback. Actions must target the ROOT tree and an exact live ID, not a draw-order index. `Semantics::update` supplies the app hook; `Semantics::action` uses existing Input/Focus. Record nodes only while accessibility is active, ending that lifetime in `on_accessibility_deactivated`. Complete screen-reader text editing and semantic coverage for every widget/application are not claimed.

### `pixelkit-ui`

Widgets draw immediately into a caller-provided `Ui`; there is no retained widget tree or global identity map. Keep persistent state such as selected rows, text field contents, dropdown state, and scroll offsets in the application. Tables support keyed selection and only draw visible rows. Scroll lists draw only rows intersecting their viewport. Layout helpers calculate flow-grid cells and visible ranges without requiring a UI runtime.

`Ui::text_field` applies committed input and async clipboard results before painting. Ctrl+A/C/X/V and Shift navigation use the same `TextFieldState`; masked fields never copy or cut their contents. Failed copies never delete selected text; late replies after an edit or focus loss are discarded. Applications can show `clipboard_error()` inline. Deliver the entire committed `KeyEvent.text`, avoiding a duplicate legacy first-character callback; `Input::set_composition` paints IME preedit, and `TextFieldState::caret_area()` returns the physical candidate anchor. Editing and password masks use Unicode extended-grapheme boundaries without normalizing stored UTF-8, including neighboring clusters changed by insertion or selection replacement. Glyph shaping and font coverage remain separate: the focused TrueType Latin/Thai stack is not full Unicode glyph or shaping coverage.
Override `PixelApp::ime_allowed()` using actual text focus (for example, whether the caret anchor exists); its default preserves legacy IME behavior. The host applies the current candidate anchor after rendering, not the previous frame's caret.

`Theme` contains semantic colors and sizing. `OperationalPalette` provides a shared appearance vocabulary, and individual controls can also be styled directly. Tooltips are requested while drawing controls and drawn after the rest of the screen so they appear above other content.

## Attribution and license

See [`ATTRIBUTION.md`](ATTRIBUTION.md) for code provenance and third-party notices. The workspace declares `MIT OR Apache-2.0` licensing; bundled sample fonts have their own SIL Open Font License terms, documented with the font files.
