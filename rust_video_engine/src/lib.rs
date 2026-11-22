pub mod config;
pub mod font8x16;
pub mod json;
pub mod rasterizer;
pub mod video;
pub mod server;

pub fn spawn_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    std::thread::spawn(f);
}

// ─── Windows CRT Shims for Self-Contained Linkage with rust-lld ──────────────

#[cfg(all(windows, target_env = "msvc"))]
#[unsafe(no_mangle)]
pub static _fltused: i32 = 0x9875;

#[cfg(all(windows, target_env = "msvc"))]
#[unsafe(no_mangle)]
pub static mut _tls_index: u32 = 0;

#[cfg(all(windows, target_env = "msvc"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __chkstk() {}

#[cfg(all(windows, target_env = "msvc"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atexit(_: *const ()) -> i32 {
    0
}

#[cfg(all(windows, target_env = "msvc"))]
#[unsafe(export_name = "??_7type_info@@6B@")]
pub static TYPE_INFO_VFTABLE: [usize; 2] = [0, 0];

use std::f32::consts::PI;
use config::{get_ansi256_palette, get_char_set, ColorMode, EngineConfig, RenderMode, ANSI16};
use rasterizer::GlyphRasterizer;

/// Stateful AI ASCII Perception Engine.
/// Implements temporal filtering, Sobel edge perception, adaptive contrast,
/// color mapping, and multi-format exporters.
pub struct AsciiAIEngine {
    pub config: EngineConfig,
    prev_lum: Option<Vec<f32>>,
    prev_chars: Option<Vec<usize>>,
    prev_mask: Option<Vec<f32>>,
    prev_dims: (usize, usize),
    frame_count: usize,
}

impl AsciiAIEngine {
    pub fn new(config: EngineConfig) -> Self {
        AsciiAIEngine {
            config,
            prev_lum: None,
            prev_chars: None,
            prev_mask: None,
            prev_dims: (0, 0),
            frame_count: 0,
        }
    }

    pub fn reset_temporal_state(&mut self) {
        self.prev_lum = None;
        self.prev_chars = None;
        self.prev_mask = None;
        self.prev_dims = (0, 0);
        self.frame_count = 0;
    }

    pub fn set_config(&mut self, config: EngineConfig) {
        self.config = config;
        self.reset_temporal_state();
    }

    /// Process a raw RGB24 frame of dimension (width, height, 3).
    /// Returns:
    ///   lines: list of strings (character grid)
    ///   colors: per-cell RGB colors of shape [rows][cols][3]
    pub fn process_frame(
        &mut self,
        rgb_data: &[u8],
        src_w: usize,
        src_h: usize,
        target_cols: Option<usize>,
        char_aspect_ratio: f32,
    ) -> (Vec<String>, Vec<Vec<[u8; 3]>>) {
        let cols = target_cols.unwrap_or(self.config.cols).max(10);
        let rows = (((cols as f32 * (src_h as f32 / src_w as f32)) * char_aspect_ratio).round() as usize).max(1);

        // 1. Area-averaged box sampling (3x3 grid sampling within each cell)
        let cell_w = src_w as f32 / cols as f32;
        let cell_h = src_h as f32 / rows as f32;

        let sample_offsets = [0.2f32, 0.5f32, 0.8f32];
        let mut sampled_rgb: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; rows * cols];

        for r in 0..rows {
            let row_offset = r * cols;
            for c in 0..cols {
                let mut acc_r = 0.0f32;
                let mut acc_g = 0.0f32;
                let mut acc_b = 0.0f32;

                for &yo in &sample_offsets {
                    let sy = ((r as f32 * cell_h + cell_h * yo) as usize).min(src_h - 1);
                    let row_stride = sy * src_w * 3;
                    for &xo in &sample_offsets {
                        let sx = ((c as f32 * cell_w + cell_w * xo) as usize).min(src_w - 1);
                        let idx = row_stride + sx * 3;
                        if idx + 2 < rgb_data.len() {
                            acc_r += rgb_data[idx] as f32;
                            acc_g += rgb_data[idx + 1] as f32;
                            acc_b += rgb_data[idx + 2] as f32;
                        }
                    }
                }

                sampled_rgb[row_offset + c] = [acc_r / 9.0, acc_g / 9.0, acc_b / 9.0];
            }
        }

        // 2. Adjustments: Brightness, Contrast, Saturation, Gamma
        let adjusted_rgb = self.apply_color_adjustments(&sampled_rgb);

        // 3. Luminance calculation (Rec. 601: 0.299 R + 0.587 G + 0.114 B)
        let mut lum = Vec::with_capacity(rows * cols);
        for p in &adjusted_rgb {
            let l = (0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]) / 255.0;
            lum.push(l.clamp(0.0, 1.0));
        }

        // 4. Adaptive Contrast / Histogram Dynamic Range Stretching
        if self.config.adaptive_contrast && lum.len() > 10 {
            let mut sorted = lum.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let idx_low = (sorted.len() as f32 * 0.02) as usize;
            let idx_high = ((sorted.len() as f32 * 0.98) as usize).min(sorted.len() - 1);
            let p_low = sorted[idx_low];
            let p_high = sorted[idx_high];
            if (p_high - p_low) > 0.05 {
                let range = p_high - p_low;
                for val in &mut lum {
                    *val = ((*val - p_low) / range).clamp(0.0, 1.0);
                }
            }
        }

        // 5. Temporal Coherence & Anti-flicker Filter
        if self.config.temporal_smoothing > 0.0 && self.prev_dims == (rows, cols) {
            if let Some(prev) = &self.prev_lum {
                let alpha = (1.0 - self.config.temporal_smoothing).clamp(0.05, 1.0);
                for i in 0..lum.len() {
                    let delta = (lum[i] - prev[i]).abs();
                    let effective_alpha = if delta > 0.12 { 1.0 } else { alpha };
                    lum[i] = lum[i] * effective_alpha + prev[i] * (1.0 - effective_alpha);
                }
            }
        }
        self.prev_lum = Some(lum.clone());

        // 6. Character Selection Ramp with Rounding Hysteresis
        let char_ramp_str = get_char_set(&self.config.char_set);
        let char_ramp: Vec<char> = char_ramp_str.chars().collect();
        let n_chars = char_ramp.len().max(1);

        let mut char_indices = Vec::with_capacity(rows * cols);
        let hysteresis = 0.35 * self.config.temporal_smoothing;

        for i in 0..lum.len() {
            let mut raw_idx = lum[i] * (n_chars - 1) as f32;
            if self.config.invert_chars {
                raw_idx = (n_chars - 1) as f32 - raw_idx;
            }

            if self.config.temporal_smoothing > 0.0 && self.prev_dims == (rows, cols) {
                if let Some(prev) = &self.prev_chars {
                    let p = prev[i] as f32;
                    let target = if raw_idx > (p + 0.5 + hysteresis) || raw_idx < (p - 0.5 - hysteresis) {
                        raw_idx.round() as usize
                    } else {
                        prev[i]
                    };
                    char_indices.push(target.min(n_chars - 1));
                    continue;
                }
            }
            char_indices.push((raw_idx.round() as usize).min(n_chars - 1));
        }
        self.prev_chars = Some(char_indices.clone());

        // 7. Initialize character grid from ramp
        let mut char_grid: Vec<char> = char_indices.iter().map(|&idx| char_ramp[idx]).collect();

        // 8. 3x3 Sobel Operator Structural Edge Perception
        if self.config.enable_edge_detection && rows > 2 && cols > 2 {
            let weight = self.config.edge_weight.clamp(0.0, 1.0);
            let effective_threshold = self.config.edge_threshold * (1.3 - 0.6 * weight);

            for r in 1..(rows - 1) {
                for c in 1..(cols - 1) {
                    let idx = r * cols + c;
                    // Sobel kernels
                    let gx = (
                        (lum[(r - 1) * cols + c + 1] + 2.0 * lum[r * cols + c + 1] + lum[(r + 1) * cols + c + 1]) -
                        (lum[(r - 1) * cols + c - 1] + 2.0 * lum[r * cols + c - 1] + lum[(r + 1) * cols + c - 1])
                    ) / 8.0;

                    let gy = (
                        (lum[(r + 1) * cols + c - 1] + 2.0 * lum[(r + 1) * cols + c] + lum[(r + 1) * cols + c + 1]) -
                        (lum[(r - 1) * cols + c - 1] + 2.0 * lum[(r - 1) * cols + c] + lum[(r - 1) * cols + c + 1])
                    ) / 8.0;

                    let mag = (gx * gx + gy * gy).sqrt();
                    if mag >= effective_threshold {
                        let mut angle = gy.atan2(gx);
                        angle = ((angle + PI) % PI).abs();

                        if angle < PI / 8.0 || angle > 7.0 * PI / 8.0 {
                            char_grid[idx] = '|';
                        } else if angle >= 3.0 * PI / 8.0 && angle <= 5.0 * PI / 8.0 {
                            char_grid[idx] = '-';
                        } else if angle >= PI / 8.0 && angle < 3.0 * PI / 8.0 {
                            char_grid[idx] = '/';
                        } else {
                            char_grid[idx] = '\\';
                        }
                    }
                }
            }
        }

        // 9. Negative Space & Subject Segmentation
        let mut out_colors: Vec<[u8; 3]> = adjusted_rgb.iter().map(|p| {
            [p[0].clamp(0.0, 255.0) as u8, p[1].clamp(0.0, 255.0) as u8, p[2].clamp(0.0, 255.0) as u8]
        }).collect();

        if self.config.render_mode != RenderMode::Normal {
            // Compute background reference from border cells
            let mut bg_acc = [0.0f32; 3];
            let mut bg_count = 0.0f32;
            for c in 0..cols {
                let top = adjusted_rgb[c];
                let btm = adjusted_rgb[(rows - 1) * cols + c];
                bg_acc[0] += top[0] + btm[0];
                bg_acc[1] += top[1] + btm[1];
                bg_acc[2] += top[2] + btm[2];
                bg_count += 2.0;
            }
            let bg_ref = [bg_acc[0] / bg_count, bg_acc[1] / bg_count, bg_acc[2] / bg_count];

            // Subject mask calculation: Color diff + Center spatial prior + YCbCr skin detection
            let cx = cols as f32 / 2.0;
            let cy = rows as f32 / 2.0;
            let mut raw_mask = Vec::with_capacity(rows * cols);

            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let p = adjusted_rgb[idx];

                    // Euclidean color distance
                    let d = (((p[0] - bg_ref[0]).powi(2) + (p[1] - bg_ref[1]).powi(2) + (p[2] - bg_ref[2]).powi(2)).sqrt() / 441.67).clamp(0.0, 1.0);

                    // Center prior
                    let dist = (((c as f32 - cx) / cx).powi(2) + ((r as f32 - cy) / cy).powi(2)).sqrt();
                    let center_prior = (1.0 - 0.65 * dist).clamp(0.0, 1.0);

                    // YCbCr skin tone
                    let cb = -0.168736 * p[0] - 0.331264 * p[1] + 0.5 * p[2] + 128.0;
                    let cr = 0.5 * p[0] - 0.418688 * p[1] - 0.081312 * p[2] + 128.0;
                    let is_skin = if (77.0..=127.0).contains(&cb) && (133.0..=173.0).contains(&cr) { 0.25f32 } else { 0.0f32 };

                    let score = d * 0.6 + center_prior * 0.35 + is_skin;
                    raw_mask.push(score);
                }
            }

            // Temporal mask smoothing
            if self.config.temporal_smoothing > 0.0 && self.prev_dims == (rows, cols) {
                if let Some(prev) = &self.prev_mask {
                    let alpha_m = (1.0 - self.config.temporal_smoothing).clamp(0.05, 1.0);
                    for i in 0..raw_mask.len() {
                        raw_mask[i] = raw_mask[i] * alpha_m + prev[i] * (1.0 - alpha_m);
                    }
                }
            }
            self.prev_mask = Some(raw_mask.clone());

            let threshold = 0.60 - (self.config.neg_space_sensitivity * 0.35);
            let bg_ramp_str = get_char_set(&self.config.neg_space_bg_char_set);
            let bg_ramp: Vec<char> = bg_ramp_str.chars().collect();
            let fg_ramp_str = get_char_set(&self.config.neg_space_fg_char_set);
            let fg_ramp: Vec<char> = fg_ramp_str.chars().collect();

            for i in 0..(rows * cols) {
                let is_subj = raw_mask[i] >= threshold;
                match self.config.render_mode {
                    RenderMode::NegativeSpace => {
                        if is_subj {
                            char_grid[i] = ' ';
                            out_colors[i] = [0, 0, 0];
                        } else {
                            if !bg_ramp.is_empty() {
                                let bg_idx = ((lum[i] * (bg_ramp.len() - 1) as f32).round() as usize).min(bg_ramp.len() - 1);
                                char_grid[i] = bg_ramp[bg_idx];
                            }
                            if self.config.neg_space_bg_brightness != 1.0 {
                                out_colors[i][0] = (out_colors[i][0] as f32 * self.config.neg_space_bg_brightness).clamp(0.0, 255.0) as u8;
                                out_colors[i][1] = (out_colors[i][1] as f32 * self.config.neg_space_bg_brightness).clamp(0.0, 255.0) as u8;
                                out_colors[i][2] = (out_colors[i][2] as f32 * self.config.neg_space_bg_brightness).clamp(0.0, 255.0) as u8;
                            }
                        }
                    }
                    RenderMode::InvertedAscii => {
                        if !is_subj {
                            char_grid[i] = ' ';
                            out_colors[i] = [0, 0, 0];
                        }
                    }
                    RenderMode::Hybrid => {
                        if is_subj && !fg_ramp.is_empty() {
                            let fg_idx = ((lum[i] * (fg_ramp.len() - 1) as f32).round() as usize).min(fg_ramp.len() - 1);
                            char_grid[i] = fg_ramp[fg_idx];
                        } else if !is_subj && !bg_ramp.is_empty() {
                            let bg_idx = ((lum[i] * (bg_ramp.len() - 1) as f32).round() as usize).min(bg_ramp.len() - 1);
                            char_grid[i] = bg_ramp[bg_idx];
                        }
                    }
                    RenderMode::Normal => {}
                }
            }
        }

        self.prev_dims = (rows, cols);
        self.frame_count += 1;

        // Convert char grid and colors into 2D lines and colors
        let mut lines = Vec::with_capacity(rows);
        let mut colors_2d = Vec::with_capacity(rows);

        for r in 0..rows {
            let row_start = r * cols;
            let row_end = row_start + cols;
            let line_chars = &char_grid[row_start..row_end];
            lines.push(line_chars.iter().collect::<String>());

            let mut row_colors = Vec::with_capacity(cols);
            for c in 0..cols {
                row_colors.push(out_colors[row_start + c]);
            }
            colors_2d.push(row_colors);
        }

        (lines, colors_2d)
    }

    fn apply_color_adjustments(&self, rgb: &[[f32; 3]]) -> Vec<[f32; 3]> {
        let mut out = rgb.to_vec();
        let b = self.config.brightness as f32;
        let c = self.config.contrast as f32;
        let s = self.config.saturation as f32;
        let g = self.config.gamma as f32;

        let contrast_factor = if c != 0.0 {
            (259.0 * (c + 255.0)) / (255.0 * (259.0 - c))
        } else {
            1.0
        };

        let sat_factor = if s != 0.0 {
            (s + 100.0) / 100.0
        } else {
            1.0
        };

        let gamma_exp = if g != 0.0 {
            if g > 0.0 { 1.0 / (1.0 + g / 100.0) } else { 1.0 + g.abs() / 100.0 }
        } else {
            1.0
        };

        for p in &mut out {
            // Brightness
            if b != 0.0 {
                p[0] += b * 2.55;
                p[1] += b * 2.55;
                p[2] += b * 2.55;
            }

            // Contrast
            if c != 0.0 {
                p[0] = contrast_factor * (p[0] - 128.0) + 128.0;
                p[1] = contrast_factor * (p[1] - 128.0) + 128.0;
                p[2] = contrast_factor * (p[2] - 128.0) + 128.0;
            }

            // Saturation
            if s != 0.0 {
                let gray = 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
                p[0] = gray + sat_factor * (p[0] - gray);
                p[1] = gray + sat_factor * (p[1] - gray);
                p[2] = gray + sat_factor * (p[2] - gray);
            }

            // Gamma
            if g != 0.0 {
                let norm_r = (p[0] / 255.0).clamp(0.0, 1.0);
                let norm_g = (p[1] / 255.0).clamp(0.0, 1.0);
                let norm_b = (p[2] / 255.0).clamp(0.0, 1.0);
                p[0] = 255.0 * norm_r.powf(gamma_exp);
                p[1] = 255.0 * norm_g.powf(gamma_exp);
                p[2] = 255.0 * norm_b.powf(gamma_exp);
            }

            p[0] = p[0].clamp(0.0, 255.0);
            p[1] = p[1].clamp(0.0, 255.0);
            p[2] = p[2].clamp(0.0, 255.0);
        }

        out
    }

    // ─── Exporters ────────────────────────────────────────────────────────────

    pub fn to_plain_text(&self, lines: &[String]) -> String {
        lines.join("\n")
    }

    pub fn to_ansi(&self, lines: &[String], colors: &[Vec<[u8; 3]>]) -> String {
        let mode = self.config.color_mode;
        if mode == ColorMode::Monochrome {
            let (pr, pg, pb) = self.config.phosphor_color;
            let prefix = format!("\x1b[38;2;{};{};{}m", pr, pg, pb);
            return lines.iter().map(|line| format!("{}{}\x1b[0m", prefix, line)).collect::<Vec<_>>().join("\n");
        }

        let mut out = Vec::with_capacity(lines.len());
        let ansi256 = get_ansi256_palette();

        for r in 0..lines.len() {
            let line_chars: Vec<char> = lines[r].chars().collect();
            let row_colors = &colors[r];
            let mut line_parts = String::new();
            let mut last_color: Option<[u8; 3]> = None;

            for c in 0..line_chars.len().min(row_colors.len()) {
                let color = row_colors[c];
                let ch = line_chars[c];

                match mode {
                    ColorMode::TrueColor => {
                        if last_color != Some(color) {
                            line_parts.push_str(&format!("\x1b[38;2;{};{};{}m", color[0], color[1], color[2]));
                            last_color = Some(color);
                        }
                    }
                    ColorMode::Grayscale => {
                        let v = (0.299 * color[0] as f32 + 0.587 * color[1] as f32 + 0.114 * color[2] as f32) as u8;
                        let g_col = [v, v, v];
                        if last_color != Some(g_col) {
                            line_parts.push_str(&format!("\x1b[38;2;{};{};{}m", v, v, v));
                            last_color = Some(g_col);
                        }
                    }
                    ColorMode::Ansi16 => {
                        let idx = nearest_color_index(&color, &ANSI16);
                        let a_col = [idx as u8, 0, 0];
                        if last_color != Some(a_col) {
                            let code = 30 + (idx % 8) + (if idx >= 8 { 60 } else { 0 });
                            line_parts.push_str(&format!("\x1b[{}m", code));
                            last_color = Some(a_col);
                        }
                    }
                    ColorMode::Ansi256 => {
                        let idx = nearest_color_index(&color, &ansi256);
                        let a_col = [idx as u8, 0, 0];
                        if last_color != Some(a_col) {
                            line_parts.push_str(&format!("\x1b[38;5;{}m", idx));
                            last_color = Some(a_col);
                        }
                    }
                    ColorMode::Monochrome => {}
                }
                line_parts.push(ch);
            }
            line_parts.push_str("\x1b[0m");
            out.push(line_parts);
        }

        out.join("\n")
    }

    pub fn to_html(&self, lines: &[String], colors: &[Vec<[u8; 3]>]) -> String {
        let mode = self.config.color_mode;
        if mode == ColorMode::Monochrome {
            let (pr, pg, pb) = self.config.phosphor_color;
            let escaped = lines.iter().map(|l| escape_html(l)).collect::<Vec<_>>().join("\n");
            return format!(
                "<pre style=\"background:#000;color:rgb({},{},{});font-family:monospace;font-size:{}px;line-height:1;margin:0;overflow:auto;\">{}</pre>",
                pr, pg, pb, self.config.font_size, escaped
            );
        }

        let mut html_rows = Vec::with_capacity(lines.len());
        for r in 0..lines.len() {
            let line_chars: Vec<char> = lines[r].chars().collect();
            let row_colors = &colors[r];
            let mut row_spans = String::new();

            for c in 0..line_chars.len().min(row_colors.len()) {
                let color = row_colors[c];
                let ch = line_chars[c];
                let ch_str = escape_html(&ch.to_string());

                if mode == ColorMode::Grayscale {
                    let v = (0.299 * color[0] as f32 + 0.587 * color[1] as f32 + 0.114 * color[2] as f32) as u8;
                    row_spans.push_str(&format!("<span style=\"color:rgb({},{},{})\">{}</span>", v, v, v, ch_str));
                } else {
                    row_spans.push_str(&format!("<span style=\"color:rgb({},{},{})\">{}</span>", color[0], color[1], color[2], ch_str));
                }
            }
            html_rows.push(row_spans);
        }

        let body = html_rows.join("\n");
        format!(
            "<pre style=\"background:#000;font-family:monospace;font-size:{}px;line-height:1;margin:0;overflow:auto;\">{}</pre>",
            self.config.font_size, body
        )
    }

    /// High-throughput software glyph rasterization to raw BGR0 image buffer
    pub fn render_to_bgr0(
        &self,
        lines: &[String],
        colors: &[Vec<[u8; 3]>],
        width: usize,
        height: usize,
        buf: &mut [u8],
    ) {
        GlyphRasterizer::render_bgr0(lines, colors, width, height, self.config.bg_color, buf);
    }
}

fn nearest_color_index(c: &[u8; 3], palette: &[[u8; 3]]) -> usize {
    let mut best_idx = 0;
    let mut best_dist = u32::MAX;
    for (i, p) in palette.iter().enumerate() {
        let dr = c[0] as i32 - p[0] as i32;
        let dg = c[1] as i32 - p[1] as i32;
        let db = c[2] as i32 - p[2] as i32;
        let dist = (dr * dr + dg * dg + db * db) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
        }
    }
    best_idx
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{get_all_presets, get_preset};

    #[test]
    fn test_core_engine_perception() {
        let mut engine = AsciiAIEngine::new(get_preset("matrix"));

        // Synthetic 320x240 image with sharp rectangular borders
        let w = 320;
        let h = 240;
        let mut frame = vec![0u8; w * h * 3];
        for y in 60..180 {
            for x in 80..240 {
                let idx = (y * w + x) * 3;
                frame[idx] = 220;
                frame[idx + 1] = 220;
                frame[idx + 2] = 220;
            }
        }

        let (lines, colors) = engine.process_frame(&frame, w, h, Some(60), 0.45);
        assert!(!lines.is_empty(), "Lines must not be empty");
        assert_eq!(lines[0].chars().count(), 60, "Expected 60 columns");
        assert_eq!(colors[0].len(), 60, "Expected 60 color entries");

        // Edge detection verification: should have directional characters
        let all_text = lines.join("");
        assert!(
            all_text.contains('|') || all_text.contains('-') || all_text.contains('/') || all_text.contains('\\'),
            "Expected Sobel operator to find sharp boundaries"
        );

        // Exporters verification
        let plain = engine.to_plain_text(&lines);
        let ansi = engine.to_ansi(&lines, &colors);
        let html = engine.to_html(&lines, &colors);

        assert!(!plain.is_empty());
        assert!(ansi.contains("\x1b[38;2;"));
        assert!(html.contains("<pre style="));
    }

    #[test]
    fn test_negative_space_subject_masking() {
        let mut engine = AsciiAIEngine::new(get_preset("negative_space"));
        let w = 320;
        let h = 240;
        let mut frame = vec![20u8; w * h * 3]; // dark bg
        // Draw centered subject
        for y in 80..160 {
            for x in 120..200 {
                let idx = (y * w + x) * 3;
                frame[idx] = 210;
                frame[idx + 1] = 160;
                frame[idx + 2] = 130;
            }
        }

        let (lines, _) = engine.process_frame(&frame, w, h, Some(60), 0.45);
        let mid_r = lines.len() / 2;
        let mid_chars: Vec<char> = lines[mid_r].chars().collect();
        let mid_c = mid_chars.len() / 2;
        assert_eq!(mid_chars[mid_c], ' ', "Subject center must be empty in negative_space mode");
    }

    #[test]
    fn test_all_presets_validity() {
        let presets = get_all_presets();
        assert!(presets.contains_key("matrix"));
        assert!(presets.contains_key("cyberpunk"));
        assert!(presets.contains_key("edge_art"));
        assert!(presets.contains_key("truecolor_cinema"));
        assert!(presets.contains_key("retro_amber"));
        assert!(presets.contains_key("braille_hd"));
        assert!(presets.contains_key("negative_space"));

        for (name, cfg) in presets {
            let mut engine = AsciiAIEngine::new(cfg);
            let frame = vec![128u8; 64 * 64 * 3];
            let (lines, _) = engine.process_frame(&frame, 64, 64, None, 0.45);
            assert!(!lines.is_empty(), "Preset {} generated empty lines", name);
        }
    }

    #[test]
    fn test_rasterizer_buffer_generation() {
        let engine = AsciiAIEngine::new(get_preset("matrix"));
        let lines = vec!["Hello World!".to_string()];
        let colors = vec![vec![[57, 255, 82]; 12]];
        let mut buf = vec![0u8; 120 * 30 * 4];
        engine.render_to_bgr0(&lines, &colors, 120, 30, &mut buf);
        // Check that at least some pixels were rendered (non-zero)
        assert!(buf.iter().any(|&b| b > 0), "Rasterizer buffer must contain drawn glyph pixels");
    }
}
