use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::AsciiAIEngine;

#[derive(Debug, Clone)]
pub struct VideoMeta {
    pub width: usize,
    pub height: usize,
    pub fps: f32,
    pub duration: f32,
    pub total_frames: usize,
    pub has_audio: bool,
    pub codec: String,
}

#[derive(Debug, Clone)]
pub struct ConversionProgress {
    pub job_id: String,
    pub status: String, // 'queued', 'processing', 'completed', 'failed', 'cancelled'
    pub current_frame: usize,
    pub total_frames: usize,
    pub percent: f32,
    pub fps_processing: f32,
    pub elapsed_seconds: f32,
    pub eta_seconds: Option<f32>,
    pub output_path: Option<String>,
    pub error_message: Option<String>,
}

impl ConversionProgress {
    pub fn new(job_id: &str, output_path: &str) -> Self {
        ConversionProgress {
            job_id: job_id.to_string(),
            status: "queued".to_string(),
            current_frame: 0,
            total_frames: 0,
            percent: 0.0,
            fps_processing: 0.0,
            elapsed_seconds: 0.0,
            eta_seconds: None,
            output_path: Some(output_path.to_string()),
            error_message: None,
        }
    }
}

pub fn find_ffmpeg() -> String {
    // Check environment variable
    if let Ok(path) = std::env::var("FFMPEG_PATH") {
        if Path::new(&path).exists() {
            return path;
        }
    }

    // Check system PATH
    if let Ok(output) = Command::new("ffmpeg").arg("-version").output() {
        if output.status.success() {
            return "ffmpeg".to_string();
        }
    }

    // Check standard known paths on Windows
    let candidates = [
        r"C:\Program Files\Wondershare\drfone\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"ffmpeg.exe",
    ];

    for c in &candidates {
        if Path::new(c).exists() {
            return c.to_string();
        }
    }

    "ffmpeg".to_string()
}

pub fn probe_video(video_path: &str) -> Result<VideoMeta, String> {
    if !Path::new(video_path).exists() {
        return Err(format!("Video file not found: {}", video_path));
    }

    let ffmpeg = find_ffmpeg();
    let output = Command::new(&ffmpeg)
        .arg("-i")
        .arg(video_path)
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut width = 640;
    let mut height = 480;
    let mut fps = 30.0f32;
    let mut duration = 0.0f32;
    let mut has_audio = false;
    let mut codec = "unknown".to_string();

    // Parse Duration: 00:01:23.45
    if let Some(pos) = stderr.find("Duration: ") {
        let sub = &stderr[pos + 10..];
        if let Some(end) = sub.find(',') {
            let dur_str = &sub[..end].trim();
            let parts: Vec<&str> = dur_str.split(':').collect();
            if parts.len() == 3 {
                let h: f32 = parts[0].parse().unwrap_or(0.0);
                let m: f32 = parts[1].parse().unwrap_or(0.0);
                let s: f32 = parts[2].parse().unwrap_or(0.0);
                duration = h * 3600.0 + m * 60.0 + s;
            }
        }
    }

    // Parse Audio
    if stderr.contains("Audio: ") {
        has_audio = true;
    }

    // Parse Video stream
    for line in stderr.lines() {
        if line.contains("Stream #") && line.contains("Video:") {
            if let Some(v_idx) = line.find("Video: ") {
                let v_info = &line[v_idx + 7..];
                let parts: Vec<&str> = v_info.split(',').collect();
                if !parts.is_empty() {
                    codec = parts[0].trim().to_string();
                }

                // Find resolution (e.g. 1920x1080)
                for part in &parts {
                    let p = part.trim();
                    if let Some(x_idx) = p.find('x') {
                        let left = &p[..x_idx].trim();
                        let right = &p[x_idx + 1..].trim();
                        // Filter right part if it has space
                        let right_cleaned = right.split_whitespace().next().unwrap_or("");
                        if let (Ok(w), Ok(h)) = (left.parse::<usize>(), right_cleaned.parse::<usize>()) {
                            if w > 0 && h > 0 {
                                width = w;
                                height = h;
                            }
                        }
                    }
                    if p.ends_with("fps") {
                        let num = p.trim_end_matches("fps").trim();
                        if let Ok(f) = num.parse::<f32>() {
                            fps = f;
                        }
                    }
                }
            }
        }
    }

    let total_frames = if duration > 0.0 && fps > 0.0 {
        (duration * fps).round() as usize
    } else {
        100
    };

    Ok(VideoMeta {
        width,
        height,
        fps,
        duration,
        total_frames,
        has_audio,
        codec,
    })
}

pub struct VideoProcessor {
    pub engine: AsciiAIEngine,
}

impl VideoProcessor {
    pub fn new(engine: AsciiAIEngine) -> Self {
        VideoProcessor { engine }
    }

    pub fn convert_video<F>(
        &mut self,
        input_path: &str,
        output_path: &str,
        target_fps: Option<f32>,
        target_resolution: Option<&str>,
        cancel_token: Option<Arc<AtomicBool>>,
        mut on_progress: F,
    ) -> Result<(), String>
    where
        F: FnMut(&ConversionProgress),
    {
        let meta = probe_video(input_path)?;
        let ffmpeg = find_ffmpeg();

        let (out_w, out_h) = match target_resolution.unwrap_or("1080p") {
            "720p" => (1280, 720),
            "1080p" => (1920, 1080),
            "1440p" | "2k" => (2560, 1440),
            "4k" => (3840, 2160),
            _ => (meta.width, meta.height),
        };

        let out_fps = target_fps.unwrap_or(meta.fps).clamp(1.0, 120.0);
        let total_frames = if meta.duration > 0.0 {
            (meta.duration * out_fps).round() as usize
        } else {
            meta.total_frames
        };

        let mut progress = ConversionProgress::new("cli_job", output_path);
        progress.total_frames = total_frames;
        progress.status = "processing".to_string();
        on_progress(&progress);

        // Temp dir for audio
        let temp_dir = std::env::temp_dir().join(format!("ascili_rs_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let temp_audio = temp_dir.join("audio.aac");

        // 1. Extract audio if present
        let mut audio_extracted = false;
        if meta.has_audio {
            let status = Command::new(&ffmpeg)
                .args(["-y", "-i", input_path, "-vn", "-c:a", "copy"])
                .arg(&temp_audio)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if status.map(|s| s.success()).unwrap_or(false) && temp_audio.exists() && temp_audio.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                audio_extracted = true;
            } else {
                // Fallback AAC encode
                let fallback = Command::new(&ffmpeg)
                    .args(["-y", "-i", input_path, "-vn", "-c:a", "aac", "-b:a", "192k"])
                    .arg(&temp_audio)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                if fallback.map(|s| s.success()).unwrap_or(false) && temp_audio.exists() && temp_audio.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                    audio_extracted = true;
                }
            }
        }

        // 2. Decoder Pipe: ffmpeg -i input -r out_fps -vf scale=out_w:out_h -f image2pipe -pix_fmt rgb24 -vcodec rawvideo -
        let mut in_proc = Command::new(&ffmpeg)
            .args([
                "-i", input_path,
                "-r", &out_fps.to_string(),
                "-vf", &format!("scale={}:{}", out_w, out_h),
                "-f", "image2pipe",
                "-pix_fmt", "rgb24",
                "-vcodec", "rawvideo",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn ffmpeg input pipe: {}", e))?;

        let in_stdout = in_proc.stdout.as_mut().ok_or_else(|| "Failed to open input pipe stdout".to_string())?;

        // 3. Encoder Pipe
        let is_webm = output_path.to_lowercase().ends_with(".webm");
        let vcodec = if is_webm { "libvpx-vp9" } else { "libx264" };
        let preset = if is_webm { "good" } else { "veryfast" };

        let mut out_cmd = Command::new(&ffmpeg);
        out_cmd.args([
            "-y",
            "-f", "rawvideo",
            "-vcodec", "rawvideo",
            "-s", &format!("{}x{}", out_w, out_h),
            "-pix_fmt", "bgr0",
            "-r", &out_fps.to_string(),
            "-i", "-",
        ]);

        if audio_extracted {
            out_cmd.args(["-i", temp_audio.to_str().unwrap(), "-c:a", "aac", "-b:a", "192k"]);
        }

        out_cmd.args([
            "-c:v", vcodec,
            "-preset", preset,
            "-crf", "18",
            "-pix_fmt", "yuv420p",
        ]);

        if !is_webm {
            out_cmd.args(["-movflags", "+faststart"]);
        }

        out_cmd.arg(output_path);
        out_cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());

        let mut out_proc = out_cmd.spawn().map_err(|e| format!("Failed to spawn ffmpeg output pipe: {}", e))?;
        let out_stdin = out_proc.stdin.as_mut().ok_or_else(|| "Failed to open output pipe stdin".to_string())?;

        let in_frame_bytes = out_w * out_h * 3;
        let out_frame_bytes = out_w * out_h * 4;

        let mut raw_in_buf = vec![0u8; in_frame_bytes];
        let mut raw_out_buf = vec![0u8; out_frame_bytes];

        let start_time = Instant::now();
        let mut frame_num = 0;
        let mut last_progress_time = Instant::now();

        loop {
            if let Some(ref token) = cancel_token {
                if token.load(Ordering::Relaxed) {
                    progress.status = "cancelled".to_string();
                    on_progress(&progress);
                    let _ = in_proc.kill();
                    let _ = out_proc.kill();
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    return Ok(());
                }
            }

            match in_stdout.read_exact(&mut raw_in_buf) {
                Ok(_) => {
                    // Process frame through AI perception engine
                    let (lines, colors) = self.engine.process_frame(
                        &raw_in_buf,
                        out_w,
                        out_h,
                        Some(self.engine.config.cols),
                        0.45,
                    );

                    // Rasterize ASCII characters into raw BGR0 buffer
                    self.engine.render_to_bgr0(&lines, &colors, out_w, out_h, &mut raw_out_buf);

                    // Write into output FFmpeg encoder pipe
                    if let Err(e) = out_stdin.write_all(&raw_out_buf) {
                        let _ = in_proc.kill();
                        let _ = out_proc.kill();
                        let _ = std::fs::remove_dir_all(&temp_dir);
                        return Err(format!("FFmpeg encoder pipe write failed: {}", e));
                    }

                    frame_num += 1;

                    if last_progress_time.elapsed().as_millis() >= 250 || frame_num == total_frames {
                        let elapsed = start_time.elapsed().as_secs_f32();
                        let fps_proc = if elapsed > 0.0 { frame_num as f32 / elapsed } else { 0.0 };
                        let pct = if total_frames > 0 { ((frame_num as f32 / total_frames as f32) * 100.0).min(100.0) } else { 0.0 };
                        let rem = if total_frames > frame_num { (total_frames - frame_num) as f32 } else { 0.0 };
                        let eta = if fps_proc > 0.0 { Some(rem / fps_proc) } else { None };

                        progress.current_frame = frame_num;
                        progress.percent = pct;
                        progress.fps_processing = fps_proc;
                        progress.elapsed_seconds = elapsed;
                        progress.eta_seconds = eta;

                        on_progress(&progress);
                        last_progress_time = Instant::now();
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break; // Reached end of video stream
                }
                Err(e) => {
                    let _ = in_proc.kill();
                    let _ = out_proc.kill();
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    return Err(format!("Error reading input video stream: {}", e));
                }
            }
        }

        // Close pipes and await encoder finish
        drop(out_proc.stdin.take());
        let _ = in_proc.wait();
        let enc_status = out_proc.wait().map_err(|e| format!("FFmpeg wait failed: {}", e))?;

        let _ = std::fs::remove_dir_all(&temp_dir);

        if !enc_status.success() {
            return Err("FFmpeg encoding failed with non-zero exit code".to_string());
        }

        progress.status = "completed".to_string();
        progress.percent = 100.0;
        progress.current_frame = frame_num;
        on_progress(&progress);

        Ok(())
    }
}
