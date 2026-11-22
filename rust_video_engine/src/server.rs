use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::{get_all_presets, get_preset};
use crate::json::JsonValue;
use crate::video::{ConversionProgress, VideoProcessor};
use crate::AsciiAIEngine;

pub struct ServerState {
    pub jobs: BTreeMap<String, JobRecord>,
    pub output_dir: PathBuf,
    pub upload_dir: PathBuf,
}

pub struct JobRecord {
    pub progress: ConversionProgress,
    pub cancel_token: Arc<AtomicBool>,
}

pub fn run_server(port: u16) {
    let output_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("converted_videos");
    let upload_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("uploaded_videos");
    let _ = std::fs::create_dir_all(&output_dir);
    let _ = std::fs::create_dir_all(&upload_dir);

    let state = Arc::new(Mutex::new(ServerState {
        jobs: BTreeMap::new(),
        output_dir,
        upload_dir,
    }));

    let addr = format!("0.0.0.0:{}", port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind server on {}: {}", addr, e);
            return;
        }
    };

    println!("============================================================");
    println!("  ASCILI Rust AI Video Engine Server running on http://127.0.0.1:{}", port);
    println!("  CORS Enabled | REST Endpoints | SSE Streaming");
    println!("============================================================");

    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let state_clone = Arc::clone(&state);
            crate::spawn_thread(move || {
                handle_client(stream, state_clone);
            });
        }
    }
}

fn handle_client(mut stream: TcpStream, state: Arc<Mutex<ServerState>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }

    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let full_path = parts[1];

    let mut headers = BTreeMap::new();
    let mut content_length = 0usize;
    let mut _content_type = String::new();

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim().to_lowercase();
            let val = line[pos + 1..].trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            } else if key == "content-type" {
                _content_type = val.clone();
            }
            headers.insert(key, val);
        }
    }

    // Auth check if ASCILI_API_KEY is set
    if let Ok(required_key) = std::env::var("ASCILI_API_KEY") {
        if !required_key.is_empty() {
            let path_only = full_path.split('?').next().unwrap_or("");
            let is_public = path_only == "/api/health" || path_only == "/api/presets" || path_only == "/" || path_only == "/index.html";
            if !is_public {
                let auth_header = headers.get("authorization").cloned().unwrap_or_default();
                let api_key_header = headers.get("x-api-key").cloned().unwrap_or_default();
                let mut authorized = false;
                if auth_header.starts_with("Bearer ") && &auth_header[7..] == required_key {
                    authorized = true;
                } else if api_key_header == required_key {
                    authorized = true;
                }

                if !authorized {
                    send_json_response(&mut stream, 401, "{\"error\":\"Unauthorized: Invalid or missing API key\"}");
                    return;
                }
            }
        }
    }

    if method == "OPTIONS" {
        send_cors_preflight(&mut stream);
        return;
    }

    let (path, _query_str) = if let Some(pos) = full_path.find('?') {
        (&full_path[..pos], &full_path[pos + 1..])
    } else {
        (full_path, "")
    };

    if method == "GET" {
        if path == "/api/health" {
            let json = "{\"status\":\"ok\",\"version\":\"2.0.0\",\"engine\":\"rust_native\",\"cairo_enabled\":true,\"active_jobs\":0}";
            send_json_response(&mut stream, 200, json);
        } else if path == "/api/presets" {
            let presets = get_all_presets();
            let mut map = BTreeMap::new();
            for (k, v) in presets {
                let mut obj = BTreeMap::new();
                obj.insert("cols".to_string(), JsonValue::Number(v.cols as f64));
                obj.insert("color_mode".to_string(), JsonValue::String(v.color_mode.as_str().to_string()));
                obj.insert("char_set".to_string(), JsonValue::String(v.char_set));
                obj.insert("enable_edge_detection".to_string(), JsonValue::Bool(v.enable_edge_detection));
                obj.insert("edge_threshold".to_string(), JsonValue::Number(v.edge_threshold as f64));
                obj.insert("temporal_smoothing".to_string(), JsonValue::Number(v.temporal_smoothing as f64));
                map.insert(k, JsonValue::Object(obj));
            }
            let resp = JsonValue::Object(map).to_string();
            send_json_response(&mut stream, 200, &resp);
        } else if path.starts_with("/api/job/") {
            let job_id = path.trim_start_matches("/api/job/").trim();
            let lock = state.lock().unwrap();
            if let Some(record) = lock.jobs.get(job_id) {
                let p = &record.progress;
                let mut obj = BTreeMap::new();
                obj.insert("job_id".to_string(), JsonValue::String(p.job_id.clone()));
                obj.insert("status".to_string(), JsonValue::String(p.status.clone()));
                obj.insert("current_frame".to_string(), JsonValue::Number(p.current_frame as f64));
                obj.insert("total_frames".to_string(), JsonValue::Number(p.total_frames as f64));
                obj.insert("percent".to_string(), JsonValue::Number(p.percent as f64));
                obj.insert("fps_processing".to_string(), JsonValue::Number(p.fps_processing as f64));
                obj.insert("elapsed_seconds".to_string(), JsonValue::Number(p.elapsed_seconds as f64));
                if let Some(eta) = p.eta_seconds {
                    obj.insert("eta_seconds".to_string(), JsonValue::Number(eta as f64));
                }
                if p.status == "completed" {
                    obj.insert("download_url".to_string(), JsonValue::String(format!("/api/download/{}", job_id)));
                }
                if let Some(ref err) = p.error_message {
                    obj.insert("error_message".to_string(), JsonValue::String(err.clone()));
                }
                let resp = JsonValue::Object(obj).to_string();
                send_json_response(&mut stream, 200, &resp);
            } else {
                send_json_response(&mut stream, 404, "{\"error\":\"Job not found\"}");
            }
        } else if path.starts_with("/api/download/") {
            let job_id = path.trim_start_matches("/api/download/").trim();
            let file_opt = {
                let lock = state.lock().unwrap();
                lock.jobs.get(job_id).and_then(|r| r.progress.output_path.clone())
            };
            if let Some(filepath) = file_opt {
                if let Ok(data) = std::fs::read(&filepath) {
                    let filename = Path::new(&filepath).file_name().and_then(|n| n.to_str()).unwrap_or("output.mp4");
                    let mime = if filename.ends_with(".webm") { "video/webm" } else { "video/mp4" };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
                        mime, data.len(), filename
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&data);
                    return;
                }
            }
            send_json_response(&mut stream, 404, "{\"error\":\"File not found\"}");
        } else if path == "/api/stream" {
            // Live Server-Sent Events (SSE) procedural ASCII plasma stream
            let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
            if stream.write_all(header.as_bytes()).is_err() {
                return;
            }

            let mut engine = AsciiAIEngine::new(get_preset("matrix"));
            let mut t = 0f32;
            let w = 160;
            let h = 90;
            let mut rgb = vec![0u8; w * h * 3];

            for _ in 0..600 {
                // Procedural plasma pattern
                for y in 0..h {
                    let fy = y as f32;
                    let stride = y * w * 3;
                    for x in 0..w {
                        let fx = x as f32;
                        let p = ((fx / 16.0 + t).sin()
                            + (fy / 8.0 + t).sin()
                            + ((fx + fy) / 16.0 + t).sin()
                            + (((fx * fx + fy * fy).sqrt() / 8.0).sin())) * 32.0 + 128.0;
                        let v = p.clamp(0.0, 255.0) as u8;
                        let idx = stride + x * 3;
                        rgb[idx] = v / 2;
                        rgb[idx + 1] = v;
                        rgb[idx + 2] = v / 3;
                    }
                }

                let (lines, _) = engine.process_frame(&rgb, w, h, Some(80), 0.45);
                let ascii_text = lines.join("\n");
                let json_event = format!("{{\"frame\":{},\"ascii\":{:?}}}", t as i32, ascii_text);
                let sse_line = format!("data: {}\n\n", json_event);

                if stream.write_all(sse_line.as_bytes()).is_err() {
                    break;
                }
                let _ = stream.flush();
                t += 0.1;
                thread::sleep(Duration::from_millis(33)); // ~30 FPS
            }
        } else if path == "/" || path == "/index.html" {
            let html = render_demo_html();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
                html.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(html.as_bytes());
        } else {
            send_json_response(&mut stream, 404, "{\"error\":\"Endpoint not found\"}");
        }
    } else if method == "POST" {
        if path.starts_with("/api/job/") && path.ends_with("/cancel") {
            let job_id = path.trim_start_matches("/api/job/").trim_end_matches("/cancel").trim();
            let mut lock = state.lock().unwrap();
            if let Some(record) = lock.jobs.get_mut(job_id) {
                record.cancel_token.store(true, Ordering::Relaxed);
                record.progress.status = "cancelled".to_string();
                send_json_response(&mut stream, 200, "{\"message\":\"Job cancellation requested\"}");
            } else {
                send_json_response(&mut stream, 404, "{\"error\":\"Job not found\"}");
            }
        } else if path == "/api/convert" {
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                send_json_response(&mut stream, 400, "{\"error\":\"Failed to read request body\"}");
                return;
            }
            let body_str = String::from_utf8_lossy(&body);
            let parsed_json = match JsonValue::parse(&body_str) {
                Ok(j) => j,
                Err(_) => {
                    send_json_response(&mut stream, 400, "{\"error\":\"Invalid JSON\"}");
                    return;
                }
            };

            let input_path = parsed_json.get("input_path").and_then(|v| v.as_str()).unwrap_or("");
            if !Path::new(input_path).exists() {
                send_json_response(&mut stream, 400, "{\"error\":\"input_path must be a valid file\"}");
                return;
            }

            let cols = parsed_json.get("cols").and_then(|v| v.as_i64()).unwrap_or(120);
            if cols < 10 || cols > 500 {
                send_json_response(&mut stream, 400, "{\"error\":\"Invalid 'cols' parameter: must be between 10 and 500\"}");
                return;
            }

            let preset_name = parsed_json.get("preset").and_then(|v| v.as_str()).unwrap_or("matrix");
            let mut config = get_preset(preset_name);
            config.cols = cols as usize;

            let job_id = format!("{:08x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() & 0xffffffff);
            let output_name = format!("ascii_{}.mp4", job_id);
            let output_path = {
                let lock = state.lock().unwrap();
                lock.output_dir.join(&output_name)
            };

            let cancel_token = Arc::new(AtomicBool::new(false));
            {
                let mut lock = state.lock().unwrap();
                lock.jobs.insert(
                    job_id.clone(),
                    JobRecord {
                        progress: ConversionProgress::new(&job_id, output_path.to_str().unwrap()),
                        cancel_token: Arc::clone(&cancel_token),
                    },
                );
            }

            // Spawn async conversion worker
            let state_worker = Arc::clone(&state);
            let job_id_clone = job_id.clone();
            let input_path_clone = input_path.to_string();
            let output_path_clone = output_path.to_str().unwrap().to_string();

            crate::spawn_thread(move || {
                let mut processor = VideoProcessor::new(AsciiAIEngine::new(config));
                let state_inner = Arc::clone(&state_worker);
                let job_id_inner = job_id_clone.clone();

                let res = processor.convert_video(
                    &input_path_clone,
                    &output_path_clone,
                    None,
                    Some("1080p"),
                    Some(cancel_token),
                    move |prog| {
                        let mut lock = state_inner.lock().unwrap();
                        if let Some(record) = lock.jobs.get_mut(&job_id_inner) {
                            record.progress = prog.clone();
                        }
                    },
                );

                if let Err(e) = res {
                    let mut lock = state_worker.lock().unwrap();
                    if let Some(record) = lock.jobs.get_mut(&job_id_clone) {
                        record.progress.status = "failed".to_string();
                        record.progress.error_message = Some(e);
                    }
                }
            });

            let resp = format!(
                "{{\"message\":\"Conversion started\",\"job_id\":\"{}\",\"status_url\":\"/api/job/{}\"}}",
                job_id, job_id
            );
            send_json_response(&mut stream, 202, &resp);
        } else {
            send_json_response(&mut stream, 404, "{\"error\":\"Endpoint not found\"}");
        }
    }
}

fn send_json_response(stream: &mut TcpStream, status: u16, body: &str) {
    let status_line = match status {
        200 => "200 OK",
        202 => "202 Accepted",
        400 => "400 Bad Request",
        401 => "401 Unauthorized",
        404 => "404 Not Found",
        _ => "500 Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, X-API-Key, X-Filename\r\n\r\n",
        status_line, body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn send_cors_preflight(stream: &mut TcpStream) {
    let header = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization, X-API-Key, X-Filename\r\nContent-Length: 0\r\n\r\n";
    let _ = stream.write_all(header.as_bytes());
}

fn render_demo_html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>ASCILI High-Speed Rust ASCII Engine</title>
    <style>
        body { background: #0a0d0e; color: #ffb000; font-family: monospace; margin: 0; padding: 20px; }
        h1 { color: #39ff52; text-shadow: 0 0 10px #39ff52; }
        pre { background: #000; padding: 12px; border: 1px solid #1a2824; color: #39ff52; font-size: 10px; line-height: 1; }
        .tag { background: #1a2824; padding: 4px 8px; border-radius: 4px; font-size: 12px; }
    </style>
</head>
<body>
    <h1>&gt;ASCILI_ RUST NATIVE VIDEO ENGINE</h1>
    <p><span class="tag">ENGINE: RUST 100% OFFLINE</span> <span class="tag">SSE STREAM ACTIVE</span></p>
    <pre id="streamPre">Connecting to live ASCII stream...</pre>
    <script>
        const es = new EventSource('/api/stream');
        const pre = document.getElementById('streamPre');
        es.onmessage = e => {
            const data = JSON.parse(e.data);
            pre.textContent = data.ascii;
        };
    </script>
</body>
</html>"#
}
