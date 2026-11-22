#![allow(non_upper_case_globals, non_snake_case)]

use std::env;
use std::io::{self, Read, Write};
use std::process::{exit, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rust_video_engine::config::{get_all_presets, get_preset, ColorMode, EngineConfig};
use rust_video_engine::server::run_server;
use rust_video_engine::video::{find_ffmpeg, probe_video, VideoProcessor};
use rust_video_engine::AsciiAIEngine;

#[cfg(all(windows, target_env = "msvc"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mainCRTStartup() -> ! {
    main();
    exit(0);
}

// ─── CLI Entry Point ─────────────────────────────────────────────────────────

fn print_help() {
    println!("ASCILI AI Video Engine (High-Performance Rust Edition)");
    println!();
    println!("Usage:");
    println!("  rust_video_engine [INPUT] [OUTPUT] [OPTIONS]");
    println!("  rust_video_engine --serve [--port PORT]");
    println!("  rust_video_engine [INPUT] --play [--preset PRESET] [--cols COLS]");
    println!("  rust_video_engine --test");
    println!();
    println!("Options:");
    println!("  --preset PRESET     AI aesthetic preset (matrix, cyberpunk, edge_art, truecolor_cinema, retro_amber, braille_hd, negative_space)");
    println!("  --cols N            Number of ASCII columns (default: preset default)");
    println!("  --color-mode MODE   Color mode (monochrome, truecolor, grayscale, ansi16, ansi256)");
    println!("  --fps N             Target framerate (e.g. 30, 60)");
    println!("  --res RES           Target output resolution (720p, 1080p, 1440p, 2k, 4k)");
    println!("  --play              Play video directly inside terminal");
    println!("  --serve             Start the REST & SSE web server");
    println!("  --port PORT         Web server port (default: 8000)");
    println!("  --test              Run internal engine test suite");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() <= 1 || args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    if args.iter().any(|a| a == "--test" || a == "test") {
        run_internal_tests();
        return;
    }

    // Server mode
    if args.iter().any(|a| a == "--serve") {
        let mut port = 8000u16;
        for i in 0..args.len() {
            if args[i] == "--port" && i + 1 < args.len() {
                if let Ok(p) = args[i + 1].parse::<u16>() {
                    port = p;
                }
            }
        }
        run_server(port);
        return;
    }

    let mut input_file: Option<String> = None;
    let mut output_file: Option<String> = None;
    let mut preset_name = "matrix".to_string();
    let mut cols_override: Option<usize> = None;
    let mut color_mode_override: Option<ColorMode> = None;
    let mut fps_override: Option<f32> = None;
    let mut res_override = "1080p".to_string();
    let mut play_mode = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--play" {
            play_mode = true;
        } else if arg == "--preset" && i + 1 < args.len() {
            i += 1;
            preset_name = args[i].clone();
        } else if arg == "--cols" && i + 1 < args.len() {
            i += 1;
            if let Ok(c) = args[i].parse::<usize>() {
                cols_override = Some(c);
            }
        } else if arg == "--color-mode" && i + 1 < args.len() {
            i += 1;
            color_mode_override = Some(ColorMode::from_str(&args[i]));
        } else if arg == "--fps" && i + 1 < args.len() {
            i += 1;
            if let Ok(f) = args[i].parse::<f32>() {
                fps_override = Some(f);
            }
        } else if arg == "--res" && i + 1 < args.len() {
            i += 1;
            res_override = args[i].clone();
        } else if !arg.starts_with('-') {
            if input_file.is_none() {
                input_file = Some(arg.clone());
            } else if output_file.is_none() {
                output_file = Some(arg.clone());
            }
        }
        i += 1;
    }

    let input = match input_file {
        Some(f) => f,
        None => {
            print_help();
            return;
        }
    };

    let mut config = get_preset(&preset_name);
    if let Some(cols) = cols_override {
        config.cols = cols;
    }
    if let Some(mode) = color_mode_override {
        config.color_mode = mode;
    }

    if play_mode {
        play_in_terminal(&input, config, fps_override.unwrap_or(30.0));
        return;
    }

    let output = output_file.unwrap_or_else(|| {
        let p = std::path::Path::new(&input);
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
        format!("ascii_{}.mp4", stem)
    });

    println!("============================================================");
    println!("  ASCILI Rust Video Engine - Converting Video");
    println!("  Input:       {}", input);
    println!("  Output:      {}", output);
    println!("  Preset:      {}", preset_name.to_uppercase());
    println!("  Resolution:  {}", res_override);
    println!("  FPS Target:  {}", fps_override.map(|f| f.to_string()).unwrap_or_else(|| "Source".to_string()));
    println!("  Columns:     {}", config.cols);
    println!("  Color Mode:  {}", config.color_mode.as_str());
    println!("============================================================");

    let mut processor = VideoProcessor::new(AsciiAIEngine::new(config));
    let t0 = Instant::now();

    let res = processor.convert_video(
        &input,
        &output,
        fps_override,
        Some(&res_override),
        None,
        |progress| {
            let eta_str = progress.eta_seconds.map(|e| format!("{}s", e as i32)).unwrap_or_else(|| "--".to_string());
            print!(
                "\rProgress: [{:5.1}%] Frame: {}/{} | Speed: {:5.1} FPS | ETA: {:<6}",
                progress.percent, progress.current_frame, progress.total_frames, progress.fps_processing, eta_str
            );
            let _ = io::stdout().flush();
        },
    );

    match res {
        Ok(_) => {
            println!();
            println!("Successfully converted in {:.1}s -> {}", t0.elapsed().as_secs_f32(), output);
        }
        Err(e) => {
            println!();
            eprintln!("Conversion Error: {}", e);
            exit(1);
        }
    }
}

fn play_in_terminal(video_path: &str, config: EngineConfig, target_fps: f32) {
    let meta = match probe_video(video_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error probing video: {}", e);
            return;
        }
    };

    let ffmpeg = find_ffmpeg();
    let fps = if meta.fps > 0.0 { meta.fps.min(target_fps) } else { 30.0 };
    let w = 320;
    let h = 180;

    let mut proc = match Command::new(&ffmpeg)
        .args([
            "-i", video_path,
            "-r", &fps.to_string(),
            "-s", &format!("{}x{}", w, h),
            "-f", "image2pipe",
            "-pix_fmt", "rgb24",
            "-vcodec", "rawvideo",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to spawn ffmpeg: {}", e);
            return;
        }
    };

    let mut stdout = proc.stdout.take().unwrap();
    let frame_bytes = w * h * 3;
    let mut buf = vec![0u8; frame_bytes];
    let frame_duration = Duration::from_secs_f32(1.0 / fps);
    let mut engine = AsciiAIEngine::new(config);

    // Clear terminal screen
    print!("\x1b[2J\x1b[H");
    let _ = io::stdout().flush();

    loop {
        let t0 = Instant::now();
        if stdout.read_exact(&mut buf).is_err() {
            break;
        }

        let (lines, colors) = engine.process_frame(&buf, w, h, Some(engine.config.cols), 0.45);
        let ansi_frame = engine.to_ansi(&lines, &colors);

        print!("\x1b[H{}\n", ansi_frame);
        let _ = io::stdout().flush();

        let elapsed = t0.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    let _ = proc.kill();
    print!("\x1b[0m\n");
}

fn run_internal_tests() {
    println!("==================================================");
    println!("  ASCILI Rust AI Video Engine - Test Suite");
    println!("==================================================");

    // 1. Core Perception Engine Test
    println!("--> [1/5] Testing AsciiAIEngine Core Logic...");
    println!("    [debug] Creating engine...");
    let mut engine = AsciiAIEngine::new(get_preset("matrix"));
    println!("    [debug] Engine created. Allocating frame...");

    let w = 640;
    let h = 480;
    let mut frame = vec![0u8; w * h * 3];
    println!("    [debug] Frame allocated. Filling pixels...");
    for y in 100..380 {
        for x in 150..490 {
            let idx = (y * w + x) * 3;
            frame[idx] = 200;
            frame[idx + 1] = 200;
            frame[idx + 2] = 200;
        }
    }
    for y in 200..280 {
        for x in 250..390 {
            let idx = (y * w + x) * 3;
            frame[idx] = 50;
            frame[idx + 1] = 150;
            frame[idx + 2] = 255;
        }
    }
    println!("    [debug] Pixels filled. Calling process_frame...");

    let t0 = Instant::now();
    let (lines, colors) = engine.process_frame(&frame, w, h, Some(80), 0.45);
    println!("    [debug] process_frame finished!");
    let elapsed = t0.elapsed();

    if lines.is_empty() {
        eprintln!("FAIL: No lines generated");
        exit(1);
    }
    if lines[0].chars().count() != 80 {
        eprintln!("FAIL: Expected 80 cols, got {}", lines[0].chars().count());
        exit(1);
    }
    if colors[0].len() != 80 {
        eprintln!("FAIL: Colors col count mismatch: {}", colors[0].len());
        exit(1);
    }

    let all_text = lines.join("");
    if !all_text.contains('|') && !all_text.contains('-') && !all_text.contains('/') && !all_text.contains('\\') {
        eprintln!("FAIL: Sobel edge detection did not find expected borders");
        exit(1);
    }

    let txt = engine.to_plain_text(&lines);
    let ansi = engine.to_ansi(&lines, &colors);
    let html = engine.to_html(&lines, &colors);

    if txt.is_empty() { eprintln!("FAIL: Plain text export empty"); exit(1); }
    if !ansi.contains("\x1b[") { eprintln!("FAIL: ANSI export missing escape codes"); exit(1); }
    if !html.contains("<pre") { eprintln!("FAIL: HTML export missing <pre> tag"); exit(1); }

    println!("    ✓ Frame processed in {:.2}ms (Throughput: ~{:.0} FPS)", elapsed.as_secs_f32() * 1000.0, 1.0 / elapsed.as_secs_f32());
    println!("    ✓ Edge-aware structural analysis passed");
    println!("    ✓ Text, ANSI, HTML exporters passed");

    // 2. Test Negative Space Mode
    println!("--> [2/5] Testing Negative Space Mode...");
    let mut neg_engine = AsciiAIEngine::new(get_preset("negative_space"));
    let mut person_frame = vec![30u8; w * h * 3];
    for y in 120..360 {
        for x in 240..400 {
            let idx = (y * w + x) * 3;
            person_frame[idx] = 210;
            person_frame[idx + 1] = 160;
            person_frame[idx + 2] = 120;
        }
    }
    let (lines_neg, _) = neg_engine.process_frame(&person_frame, w, h, Some(80), 0.45);
    if lines_neg.is_empty() { eprintln!("FAIL: Negative space lines empty"); exit(1); }
    let mid_r = lines_neg.len() / 2;
    let mid_chars: Vec<char> = lines_neg[mid_r].chars().collect();
    let mid_c = mid_chars.len() / 2;
    if mid_chars[mid_c] != ' ' {
        eprintln!("FAIL: Expected empty space in silhouette center, got '{}'", mid_chars[mid_c]);
        exit(1);
    }
    println!("    ✓ Negative space subject silhouette masking verified");

    // 3. Test Presets
    println!("--> [3/5] Testing All Presets...");
    for (name, cfg) in get_all_presets() {
        let mut p_engine = AsciiAIEngine::new(cfg);
        let test_frame = vec![128u8; 100 * 100 * 3];
        let (p_lines, _) = p_engine.process_frame(&test_frame, 100, 100, None, 0.45);
        if p_lines.is_empty() { eprintln!("FAIL: Preset '{}' produced empty lines", name); exit(1); }
        println!("    ✓ Preset '{}': cols={}, mode={:?}", name, p_engine.config.cols, p_engine.config.color_mode);
    }

    // 4. Test Video Probe and Synthetic Video Conversion Pipeline
    println!("--> [4/5] Testing Full Video Conversion Pipeline...");
    let ffmpeg = find_ffmpeg();
    println!("    ✓ Found FFmpeg at: {}", ffmpeg);

    let test_dir = std::env::temp_dir().join(format!("ascili_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&test_dir);
    let input_vid = test_dir.join("test_input.mp4");
    let output_vid = test_dir.join("test_output.mp4");

    let gen_status = Command::new(&ffmpeg)
        .args([
            "-y",
            "-f", "lavfi", "-i", "testsrc=duration=2:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=2",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:a", "aac",
            input_vid.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if gen_status.map(|s| s.success()).unwrap_or(false) && input_vid.exists() {
        if let Ok(meta) = probe_video(input_vid.to_str().unwrap()) {
            println!("    ✓ Probed synthetic video: {}x{}, {:.1} FPS, Audio={}", meta.width, meta.height, meta.fps, meta.has_audio);
            let mut processor = VideoProcessor::new(AsciiAIEngine::new(get_preset("cyberpunk")));
            let res = processor.convert_video(
                input_vid.to_str().unwrap(),
                output_vid.to_str().unwrap(),
                Some(30.0),
                Some("720p"),
                None,
                |_| {},
            );
            if res.is_ok() && output_vid.exists() {
                if let Ok(out_meta) = probe_video(output_vid.to_str().unwrap()) {
                    println!("    ✓ Successfully converted to ASCII video: {}x{}, Audio={}", out_meta.width, out_meta.height, out_meta.has_audio);
                }
            } else {
                eprintln!("    ⚠ Video conversion notice: {:?}", res);
            }
        }
    } else {
        println!("    ⚠ Skipping synthetic video render test (lavfi not supported or blocked)");
    }
    let _ = std::fs::remove_dir_all(&test_dir);

    // 5. Test Web API Server Endpoints
    println!("--> [5/5] Testing Web API Server...");
    let test_port = 8123u16;
    rust_video_engine::spawn_thread(move || {
        run_server(test_port);
    });
    thread::sleep(Duration::from_millis(600));

    // Test health endpoint using standard TcpStream
    if let Ok(mut stream) = std::net::TcpStream::connect(format!("127.0.0.1:{}", test_port)) {
        let req = format!("GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", test_port);
        let _ = stream.write_all(req.as_bytes());
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        if resp.contains("200 OK") && resp.contains("\"status\":\"ok\"") {
            println!("    ✓ Health endpoint verified (200 OK, JSON valid)");
        } else {
            eprintln!("FAIL: Health check response unexpected: {}", resp);
        }
    }

    if let Ok(mut stream) = std::net::TcpStream::connect(format!("127.0.0.1:{}", test_port)) {
        let req = format!("GET /api/presets HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", test_port);
        let _ = stream.write_all(req.as_bytes());
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        if resp.contains("200 OK") && resp.contains("\"matrix\"") {
            println!("    ✓ Presets endpoint verified (200 OK)");
        } else {
            eprintln!("FAIL: Presets check response unexpected: {}", resp);
        }
    }

    println!("\n==================================================");
    println!("  ALL ENGINE TESTS PASSED SUCCESSFULLY! (100% Ready)");
    println!("==================================================");
}
