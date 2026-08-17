# pixelkit

A small, dependency-light toolkit for software-rendered desktop GUIs in Rust:
a `u32` pixel buffer, an exact-area anti-aliasing rasterizer, TrueType
parsing and OpenType shaping, a string cache, a winit host with HiDPI and a
presenter seam, and immediate-mode widgets.

| Crate | External deps | What |
|---|---|---|
| `pixelkit-raster` | none | `WindowBuffer`, `Painter` (clip stack, fills, blends), `RasterKernel` (analytic coverage), `Path` (lines/quads/cubics, circles, rounded rects, strokes), PNG writer |
| `pixelkit-text` | none | `Face::from_bytes` (glyf TrueType), GSUB/GPOS (ccmp/liga/kern/mark/mkmk), `FontSet` fallback chains, `TextStyle` (face, size, tracking), `TextCache` |
| `pixelkit-shell` | winit 0.30, softbuffer 0.4 | `PixelApp` trait, `run_app`, `Waker`, `Scale`, `Presenter` + `SoftbufferPresenter`, `Input`, `FrameTimer` |
| `pixelkit-ui` | none | `Ui` (labels, buttons, tabs, segmented, stepper, tracks, dots, pills, panels, tables, text fields, scroll lists, tooltips) |

`unsafe_code = "forbid"` throughout. Applications embed their own fonts.

```sh
cargo test --workspace
cargo run -p pixelkit-ui --example gallery --features test-fonts   # target/gallery@1x.png
cargo run -p pixelkit-ui --example window  --features test-fonts   # live
cargo run -p pixelkit-text --example glyph_sheet --features test-fonts -- [font.ttf ...]
PIXELKIT_FRAME_LOG=1 <your app>   # p50/p95/max per phase every 2 s
```

See `ATTRIBUTION.md` for where the code came from.
