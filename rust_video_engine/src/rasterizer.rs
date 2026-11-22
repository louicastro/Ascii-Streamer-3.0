use crate::font8x16::get_glyph;

/// High-throughput software glyph rasterizer.
/// Renders an ASCII text grid with per-cell colors into a raw pixel buffer (BGR0 or RGB24).
/// This allows direct hardware-rate video encoding without depending on external graphics libraries (Cairo/Skia).
pub struct GlyphRasterizer;

impl GlyphRasterizer {
    /// Render to BGR0 buffer (standard for FFmpeg `-pix_fmt bgr0` / Cairo format)
    pub fn render_bgr0(
        lines: &[String],
        colors: &[Vec<[u8; 3]>],
        width: usize,
        height: usize,
        bg_color: (u8, u8, u8),
        buf: &mut [u8],
    ) {
        let expected_size = width * height * 4;
        if buf.len() < expected_size {
            return;
        }

        let rows = lines.len();
        if rows == 0 {
            buf[..expected_size].fill(0);
            return;
        }
        let cols = lines[0].chars().count();
        if cols == 0 {
            buf[..expected_size].fill(0);
            return;
        }

        let cell_w = width as f32 / cols as f32;
        let cell_h = height as f32 / rows as f32;

        for r in 0..rows {
            let row_str: Vec<char> = lines[r].chars().collect();
            let row_colors = &colors[r];
            let y_start = (r as f32 * cell_h).round() as usize;
            let y_end = (((r + 1) as f32 * cell_h).round() as usize).min(height);
            let ch_h = y_end - y_start;
            if ch_h == 0 {
                continue;
            }

            for c in 0..cols.min(row_str.len()).min(row_colors.len()) {
                let ch = row_str[c];
                let [cr, cg, cb] = row_colors[c];
                let glyph = get_glyph(ch);

                let x_start = (c as f32 * cell_w).round() as usize;
                let x_end = (((c + 1) as f32 * cell_w).round() as usize).min(width);
                let ch_w = x_end - x_start;
                if ch_w == 0 {
                    continue;
                }

                for dy in 0..ch_h {
                    let py = y_start + dy;
                    let font_y = (dy * 16) / ch_h;
                    let glyph_byte = glyph[font_y.min(15)];

                    let row_stride = py * width * 4;

                    for dx in 0..ch_w {
                        let px = x_start + dx;
                        let font_x = (dx * 8) / ch_w;
                        let bit = (glyph_byte >> (7 - font_x.min(7))) & 1;

                        let idx = row_stride + px * 4;
                        if bit == 1 {
                            buf[idx] = cb;     // B
                            buf[idx + 1] = cg; // G
                            buf[idx + 2] = cr; // R
                            buf[idx + 3] = 255;
                        } else {
                            buf[idx] = bg_color.2;
                            buf[idx + 1] = bg_color.1;
                            buf[idx + 2] = bg_color.0;
                            buf[idx + 3] = 255;
                        }
                    }
                }
            }
        }
    }
}
