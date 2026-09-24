# Provenance

pixelkit is extracted from GUI code written for other in-house projects, all
by the same author, and generalised. Where a module is a near-verbatim copy,
its history lives in the source repository:

| pixelkit module | Source |
|---|---|
| `pixelkit-raster/src/painter.rs` | `Kitchen-Concept/restaurant-pos/crates/pos-client-ui/src/paint.rs` |
| `pixelkit-raster/src/blend.rs` | `Kitchen-Concept/restaurant-pos/crates/pos-simd/src/scalar.rs` (the SIMD levels were left behind: pixelkit forbids `unsafe`) |
| `pixelkit-raster/src/kernel.rs` | `Kitchen-Concept/restaurant-pos/crates/thai-text/src/raster.rs`, itself a trim of `Lege-ecosystem/lege-pdf` `pdf-render-cpu::raster` |
| `pixelkit-raster/src/{path,shapes,color,png}.rs` | new; `hline/vline/blend_rect` follow `Kitchen-Simulation/crates/kitchen-ui/src/paint.rs` |
| `pixelkit-text/src/{sfnt,layout}.rs` | `thai-text` verbatim |
| `pixelkit-text/src/{font,text}.rs` | `thai-text`, generalised from two embedded faces to `Face::from_bytes` + `FontSet`, with letter-spacing added |
| `pixelkit-text/src/cache.rs` | `pos-client-ui/src/text.rs`, keyed by `TextStyle` |
| `pixelkit-text/test-fonts/` | Noto Sans / Noto Sans Thai (OFL 1.1), test fixtures and examples only |
| `pixelkit-shell/src/{shell,input}.rs` | `pos-client-ui/src/{shell,input}.rs`, with HiDPI scale tracking and the `Presenter` seam added; the seam follows `Lege-ecosystem/lege-viewer/src/present/mod.rs` |
| `pixelkit-ui/src/widget.rs` | `pos-client-ui/src/widget.rs` minus the recipe card |
| `pixelkit-ui/src/{chrome,list,tooltip}.rs` | new |
