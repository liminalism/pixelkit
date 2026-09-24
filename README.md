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
- A `winit` 0.30 desktop host with `softbuffer` presentation, redraw pacing, HiDPI scale conversion, keyboard and pointer input, scroll and magnify gestures, IME events, clipboard access, and system appearance helpers.
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
| [`pixelkit-shell`](crates/pixelkit-shell) | Window host, input translation, scale handling and frame presentation | `winit`, `softbuffer`, `pixelkit-raster` |
| [`pixelkit-ui`](crates/pixelkit-ui) | Immediate-mode widgets and layout utilities | The other three workspace crates |

The raster, text, and UI crates have no third-party runtime dependencies of their own. The shell uses `winit` and `softbuffer`; its optional `accessibility` feature enables AccessKit support. On macOS, clipboard and platform helpers use safe `objc2` wrappers. Applications provide and embed their own production fonts.

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

An application implements `pixelkit_shell::PixelApp`. The shell calls its rendering and event hooks, provides a `WindowBuffer`, and presents completed frames. The application can keep its widget state, `Input`, `TextCache`, and `RasterKernel` as ordinary fields. The UI example in [`crates/pixelkit-ui/examples/window.rs`](crates/pixelkit-ui/examples/window.rs) shows this arrangement.

At a high level, a render pass creates a `Painter` over the supplied buffer and a `Ui` over the painter, text cache, input state, theme, scale, and raster kernel. Draw the desired controls, update application-owned state from their return values, and call `Input::end_frame()` when frame input has been consumed. Coordinates passed to `Ui` are logical pixels and are converted using the supplied `Scale`.

For applications that need a different display path, implement the shell's `Presenter` interface and use `run_app_with_presenter`. The default `run_app` path uses the built-in `SoftbufferPresenter`.

## Crate notes

### `pixelkit-raster`

`WindowBuffer` stores one `0x00RRGGBB` pixel per location. `Painter` provides clipped pixel writes, blending, rectangles, rounded rectangles, rules, and path fills. `RasterKernel` computes analytic coverage for paths, which gives vector edges and text the same anti-aliasing approach. `Bitmap` uses `0xAARRGGBB` straight-alpha pixels; `Painter::blit` composites and optionally scales a bitmap.

The PNG decoder handles 8-bit grayscale, RGB, indexed, and RGBA images, including `tRNS` transparency and Adam7 interlacing. It checks chunk CRCs and bounds and limits declared dimensions before allocating. The encoder writes a `WindowBuffer` as PNG.

### `pixelkit-text`

Faces are parsed from application-provided static bytes with `Face::from_bytes` and registered in a `FontSet`. A `TextStyle` selects a face, pixel size, and letter spacing. The shaping and rendering functions can be used directly, or `TextCache` can cache layout and rendered coverage for repeated UI strings.

The implementation targets the TrueType outlines and OpenType substitutions and positioning used by this project. GSUB support includes single, multiple, alternate, ligature, and chained contextual substitutions; GPOS support includes pair positioning and mark attachment. Unsupported lookup types are skipped. This is a focused text stack, not a claim of complete Unicode shaping coverage.

### `pixelkit-shell`

`PixelApp` exposes hooks for drawing, ticks, keyboard events, IME input, cursor and mouse events, scrolling, magnification, theme changes, and exit handling. The shell translates platform events into these hooks, tracks physical and logical scale, and presents the software-rendered buffer. `Waker` can request a redraw. Clipboard helpers use the system pasteboard on macOS and an in-memory fallback elsewhere.

Enable `accessibility` on `pixelkit-shell` to publish an application-supplied AccessKit tree and receive accessibility actions. The widgets do not automatically construct a semantic accessibility tree; applications that enable this feature provide that tree through the app hook.

### `pixelkit-ui`

Widgets draw immediately into a caller-provided `Ui`; there is no retained widget tree or global identity map. Keep persistent state such as selected rows, text field contents, dropdown state, and scroll offsets in the application. Tables support keyed selection and only draw visible rows. Scroll lists draw only rows intersecting their viewport. Layout helpers calculate flow-grid cells and visible ranges without requiring a UI runtime.

`Theme` contains semantic colors and sizing. `OperationalPalette` provides a shared appearance vocabulary, and individual controls can also be styled directly. Tooltips are requested while drawing controls and drawn after the rest of the screen so they appear above other content.

## Attribution and license

See [`ATTRIBUTION.md`](ATTRIBUTION.md) for code provenance and third-party notices. The workspace declares `MIT OR Apache-2.0` licensing; bundled sample fonts have their own SIL Open Font License terms, documented with the font files.
