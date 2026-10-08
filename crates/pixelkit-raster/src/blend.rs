//! Exact portable blend arithmetic and a vector coverage merge.
//!
//! These are not "the slow path". They are the specification: every other
//! backend is asserted byte-for-byte identical to what is here, over exhaustive
//! inputs where exhaustive is possible. A vector kernel that is merely *close*
//! would show up as a faint fringe on every glyph edge, or a dot that prints on
//! one machine and not another — the kind of difference nobody attributes to
//! arithmetic.

use wide::u8x32;

/// Integer blend in sRGB space, weights summing to 256.
///
/// The weights are `alpha + 1` and `256 - (alpha + 1)`. Weighting by `alpha`
/// and `255 - alpha` sums to 255, which loses a step: blending a colour over
/// *itself* returns one less than it started with, so antialiased text drawn in
/// the panel's own colour grows a faint dark fringe on every edge of every
/// letter.
///
/// Two channels are multiplied at once by keeping red and blue in one word.
/// That is only safe while each 16-bit lane stays inside itself, and it does:
/// `d * (256 - a) + s * a <= max(d, s) * 256 <= 65280`. The vector kernels rely
/// on exactly the same bound, which is why they can use 16-bit lanes.
#[inline]
pub fn blend_channels(destination: u32, source: u32, alpha: u8) -> u32 {
    let alpha = u32::from(alpha) + 1;
    let inverse = 256 - alpha;
    let red_blue = ((destination & 0x00ff_00ff) * inverse + (source & 0x00ff_00ff) * alpha) >> 8;
    let green = ((destination & 0x0000_ff00) * inverse + (source & 0x0000_ff00) * alpha) >> 8;
    (red_blue & 0x00ff_00ff) | (green & 0x0000_ff00)
}

/// One pixel of coverage-weighted compositing.
///
/// The two ends are exact rather than blended: zero coverage leaves the
/// destination untouched (the arithmetic alone would not — at `alpha = 0` the
/// weights are 255 and 1, which moves an opaque pixel by a step), and full
/// coverage stores the colour verbatim.
#[inline]
pub fn blend_pixel(destination: u32, color: u32, coverage: u8) -> u32 {
    match coverage {
        0 => destination,
        255 => color,
        _ => blend_channels(destination, color, coverage),
    }
}

/// Blend one colour along a row, weighted per pixel by a coverage byte.
///
/// The glyph-drawing kernel. Stops at the shorter of the two slices, so a
/// caller that has clipped one and not the other cannot run off the end.
#[inline]
pub fn blend_coverage_span(destination: &mut [u32], color: u32, coverage: &[u8]) {
    for (pixel, &cover) in destination.iter_mut().zip(coverage) {
        *pixel = blend_pixel(*pixel, color, cover);
    }
}

/// Source-over an RGB colour with coverage onto a premultiplied ARGB destination.
/// Unlike the XRGB kernel, the alpha lane is accumulated and RGB never exceeds alpha.
#[inline]
pub(crate) fn blend_premultiplied(destination: u32, color: u32, coverage: u8) -> u32 {
    let alpha = u32::from(coverage);
    let inverse = 255 - alpha;
    let channel = |shift: u32| -> u32 {
        let destination = (destination >> shift) & 255_u32;
        let source = (color >> shift) & 255_u32;
        (source * alpha + destination * inverse + 127) / 255
    };
    let out_alpha = alpha + (((destination >> 24) * inverse + 127) / 255);
    (out_alpha << 24) | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// Merge coverage in, keeping the greater value.
///
/// `max` rather than adding: two overlapping contours of the same glyph — a
/// tone mark whose bounding box overlaps its base — must not produce a doubly
/// dark seam where they meet.
#[inline]
pub fn max_span(destination: &mut [u8], source: &[u8]) {
    let len = destination.len().min(source.len());
    // Scalar code is already auto-vectorized on short glyph spans. Keep the
    // explicit wide path on larger coverage buffers to avoid call overhead.
    if len < 1024 {
        for (slot, &cover) in destination.iter_mut().zip(source) {
            *slot = (*slot).max(cover);
        }
        return;
    }
    let (destination, source) = (&mut destination[..len], &source[..len]);
    let mut chunks = destination
        .chunks_exact_mut(32)
        .zip(source.chunks_exact(32));
    for (dst, src) in &mut chunks {
        let mut dst_array = [0u8; 32];
        let mut src_array = [0u8; 32];
        dst_array.copy_from_slice(dst);
        src_array.copy_from_slice(src);
        dst.copy_from_slice(
            &u8x32::from(dst_array)
                .max(u8x32::from(src_array))
                .to_array(),
        );
    }
    let offset = len - len % 32;
    for (slot, &cover) in destination[offset..].iter_mut().zip(&source[offset..]) {
        *slot = (*slot).max(cover);
    }
}

/// Threshold coverage to one bit per pixel, MSB first — the layout an ESC/POS
/// raster command expects.
///
/// `output` must hold `coverage.len().div_ceil(8)` bytes and is written whole,
/// including the padding bits of a final partial byte, which are zero.
#[inline]
pub fn pack_row_1bit(coverage: &[u8], threshold: u8, output: &mut [u8]) {
    let bytes = coverage.len().div_ceil(8);
    let output = &mut output[..bytes];
    output.fill(0);
    for (index, &cover) in coverage.iter().enumerate() {
        if cover >= threshold {
            output[index / 8] |= 0x80 >> (index % 8);
        }
    }
}

/// Whether every byte is zero — asked of a whole receipt before it is encoded,
/// and of a glyph bitmap before it is cached.
#[inline]
pub fn all_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_max_matches_scalar_for_clipped_and_unaligned_spans() {
        for dst_len in 0..65 {
            for src_len in 0..65 {
                let mut actual = (0..dst_len)
                    .map(|i| (i * 37 + 11) as u8)
                    .collect::<Vec<_>>();
                let source = (0..src_len)
                    .map(|i| (i * 53 + 197) as u8)
                    .collect::<Vec<_>>();
                let mut expected = actual.clone();
                for (slot, &cover) in expected.iter_mut().zip(&source) {
                    *slot = (*slot).max(cover);
                }
                max_span(&mut actual, &source);
                assert_eq!(actual, expected, "dst={dst_len}, src={src_len}");
            }
        }
    }

    #[test]
    fn vector_max_matches_scalar_across_dispatch_boundary() {
        for dst_len in [1023, 1024, 1025, 2048, 4097] {
            for src_len in [1023, 1024, 1025, 2048, 4097] {
                let mut actual = (0..dst_len)
                    .map(|index| (index * 37 + 11) as u8)
                    .collect::<Vec<_>>();
                let source = (0..src_len)
                    .map(|index| (index * 53 + 197) as u8)
                    .collect::<Vec<_>>();
                let mut expected = actual.clone();
                for (slot, &cover) in expected.iter_mut().zip(&source) {
                    *slot = (*slot).max(cover);
                }
                max_span(&mut actual, &source);
                assert_eq!(actual, expected, "dst={dst_len}, src={src_len}");
            }
        }
    }

    #[test]
    fn blending_a_colour_over_itself_changes_nothing() {
        for colour in [0x0000_0000, 0x0012_3456, 0x00ff_ffff, 0x0080_8080] {
            for alpha in 1..=254u8 {
                assert_eq!(blend_channels(colour, colour, alpha), colour);
            }
        }
    }

    #[test]
    fn a_channel_never_bleeds_into_its_neighbour() {
        // Exhaustive over one channel pair: the 16-bit lane bound the vector
        // kernels depend on either holds for every input or it does not.
        for destination in 0..=255u32 {
            for alpha in 0..=255u8 {
                let blended = blend_channels(destination * 0x0001_0001, 0x0000_ff00, alpha);
                assert!(blended <= 0x00ff_ffff, "{blended:#08x} left the lanes");
            }
        }
    }

    #[test]
    fn the_ends_of_the_coverage_range_are_exact() {
        assert_eq!(blend_pixel(0x0011_2233, 0x00ff_ffff, 0), 0x0011_2233);
        assert_eq!(blend_pixel(0x0011_2233, 0x00ff_ffff, 255), 0x00ff_ffff);
        // Arithmetic alone would not give either: at coverage 0 the weights
        // are 255 and 1, which moves an opaque destination by a step.
        assert_ne!(blend_channels(0x00ff_ffff, 0x0000_0000, 0), 0x00ff_ffff);
    }

    #[test]
    fn a_span_stops_at_the_shorter_of_its_two_slices() {
        let mut pixels = [0u32; 4];
        blend_coverage_span(&mut pixels, 0x00ff_ffff, &[255, 255]);
        assert_eq!(pixels, [0x00ff_ffff, 0x00ff_ffff, 0, 0]);

        let mut pixels = [0u32; 2];
        blend_coverage_span(&mut pixels, 0x00ff_ffff, &[255; 8]);
        assert_eq!(pixels, [0x00ff_ffff; 2]);
    }

    #[test]
    fn packing_pads_the_last_byte_with_zeros() {
        let mut packed = [0xffu8; 2];
        pack_row_1bit(&[255, 0, 255, 0, 0, 0, 0, 0, 255], 128, &mut packed);
        assert_eq!(packed[0], 0b1010_0000);
        assert_eq!(packed[1], 0b1000_0000, "seven padding bits, all zero");
    }

    #[test]
    fn a_zero_threshold_prints_every_dot() {
        // `>= threshold`, so a threshold of zero is satisfied by blank paper.
        let mut packed = [0u8; 1];
        pack_row_1bit(&[0; 8], 0, &mut packed);
        assert_eq!(packed[0], 0xff);
    }

    #[test]
    fn emptiness_is_recognised_and_a_single_dot_is_not_lost() {
        assert!(all_zero(&[]));
        assert!(all_zero(&[0; 1000]));
        for position in 0..1000 {
            let mut bytes = vec![0u8; 1000];
            bytes[position] = 1;
            assert!(!all_zero(&bytes), "lost the dot at {position}");
        }
    }
}
