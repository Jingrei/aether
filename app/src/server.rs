//! Local control server (std only): serves the embedded UI, a JSON command API, and a Server-Sent
//! Events stream with state changes and live telemetry (meters, spectrum, DSP load).
//! Bound to 127.0.0.1 only.

use crate::state::{App, Command};
use aether_engine::Meters;
use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

const INDEX: &str = include_str!("../../ui/index.html");
const APP_JS: &str = include_str!("../../ui/app.js");
const STYLE: &str = include_str!("../../ui/style.css");

pub const PORT: u16 = 47820;
const MAX_BODY: usize = 1 << 20;

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Requests from other web origins are rejected so web pages cannot drive the local API.
    fn same_origin(&self) -> bool {
        match self.header("origin") {
            None => true,
            Some(o) => o == format!("http://127.0.0.1:{PORT}") || o == format!("http://localhost:{PORT}"),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
        if headers.len() > 100 {
            break;
        }
    }
    let len: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0; len.min(MAX_BODY)];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, headers, body })
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

pub fn serve(listener: TcpListener, app: Arc<App>) {
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        let _ = std::thread::Builder::new().name("aether-http".into()).spawn(move || {
            let _ = handle(stream, &app);
        });
    }
}

fn handle(mut stream: TcpStream, app: &App) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let req = read_request(&mut stream)?;
    if !req.same_origin() {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden");
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", INDEX.as_bytes()),
        ("GET", "/app.js") => respond(&mut stream, "200 OK", "text/javascript; charset=utf-8", APP_JS.as_bytes()),
        ("GET", "/style.css") => respond(&mut stream, "200 OK", "text/css; charset=utf-8", STYLE.as_bytes()),
        ("GET", "/api/state") => respond(&mut stream, "200 OK", "application/json", &serde_json::to_vec(&app.snapshot())?),
        ("POST", "/api/cmd") => {
            // A custom header forces a CORS preflight, which this server never approves.
            if req.header("x-aether").is_none() {
                return respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden");
            }
            match serde_json::from_slice::<Command>(&req.body) {
                Err(e) => respond(&mut stream, "400 Bad Request", "text/plain", e.to_string().as_bytes()),
                Ok(cmd) => match app.apply(cmd) {
                    Ok(()) => respond(&mut stream, "200 OK", "application/json", &serde_json::to_vec(&app.snapshot())?),
                    Err(e) => respond(&mut stream, "400 Bad Request", "text/plain", e.as_bytes()),
                },
            }
        }
        ("GET", "/events") => events(stream, app),
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// Server-Sent Events: pushes the state whenever it changes, and telemetry at 20 Hz.
fn events(mut stream: TcpStream, app: &App) -> std::io::Result<()> {
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n")?;
    let mut seen = u64::MAX;
    let mut analyzer = Analyzer::new();
    while !app.quit.load(Ordering::Relaxed) {
        let v = app.version();
        if v != seen {
            seen = v;
            let json = serde_json::to_string(&app.snapshot())?;
            write!(stream, "event: state\ndata: {json}\n\n")?;
        }
        let t = analyzer.telemetry(&app.engine.meters);
        write!(stream, "event: telemetry\ndata: {t}\n\n")?;
        stream.flush()?;
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

const FFT: usize = 4096;
const BANDS: usize = 64;

struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex32>,
    smooth: Vec<f32>,
    edges: Vec<usize>,
}

impl Analyzer {
    fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
        let window = (0..FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT as f32).cos()).collect();
        // Log-spaced band edges from 20 Hz to 20 kHz (assuming 48 kHz).
        let edges = (0..=BANDS)
            .map(|b| {
                let f = 20.0 * 1000f32.powf(b as f32 / BANDS as f32);
                ((f / 48000.0 * FFT as f32).round() as usize).clamp(1, FFT / 2)
            })
            .collect();
        Self { spectrum: fft.make_output_vec(), input: vec![0.0; FFT], fft, window, smooth: vec![-90.0; BANDS], edges }
    }

    fn telemetry(&mut self, meters: &Meters) -> serde_json::Value {
        meters.tap.snapshot(&mut self.input);
        for (x, w) in self.input.iter_mut().zip(&self.window) {
            *x *= w;
        }
        let _ = self.fft.process(&mut self.input, &mut self.spectrum);
        let norm = 2.0 / (FFT as f32 * 0.5);
        for b in 0..BANDS {
            let (lo, hi) = (self.edges[b], self.edges[b + 1].max(self.edges[b] + 1));
            let p = self.spectrum[lo..hi.min(self.spectrum.len())].iter().map(|c| c.norm_sqr()).fold(0.0f32, f32::max);
            let db = 10.0 * (p * norm * norm).max(1e-12).log10();
            let s = &mut self.smooth[b];
            *s = if db > *s { db } else { *s + (db - *s) * 0.25 };
        }
        let (inp, out) = meters.take_peaks();
        let to_db = |v: f32| (20.0 * v.max(1e-6).log10()).max(-90.0);
        serde_json::json!({
            "in": [to_db(inp[0]), to_db(inp[1])],
            "out": [to_db(out[0]), to_db(out[1])],
            "load": meters.load(),
            "spectrum": self.smooth.iter().map(|&v| ((v as f64) * 10.0).round() / 10.0).collect::<Vec<_>>(),
        })
    }
}
