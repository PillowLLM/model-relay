//! 图形验证码：手写点阵 PNG（4 位数字 + 噪点）。
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::Json;
use rand::Rng;
use serde_json::json;

use crate::error::ApiResult;
use crate::AppState;

pub struct CaptchaStore {
    map: Mutex<std::collections::HashMap<String, (String, Instant)>>,
}

impl CaptchaStore {
    pub fn new() -> Self {
        Self { map: Mutex::new(std::collections::HashMap::new()) }
    }
    pub fn verify_and_consume(&self, id: &str, answer: &str) -> bool {
        let mut m = self.map.lock().unwrap();
        if let Some((code, t)) = m.remove(id) {
            if t.elapsed() < Duration::from_secs(300) {
                return code.eq_ignore_ascii_case(answer);
            }
        }
        false
    }
}

impl Default for CaptchaStore {
    fn default() -> Self {
        Self::new()
    }
}

/// GET /api/auth/captcha → {id, image(data URL)}
pub async fn get_captcha(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let id = format!("{:016x}", rand::thread_rng().gen::<u64>());
    let mut rng = rand::thread_rng();
    let code: String = (0..4).map(|_| rng.gen_range(0..10).to_string()).collect();
    let png = render_png(&code);
    let b64 = base64_encode(&png);
    let data_url = format!("data:image/png;base64,{b64}");
    state.captcha.map.lock().unwrap().insert(id.clone(), (code, Instant::now()));
    Ok(Json(json!({ "id": id, "image": data_url })))
}

/// 极简 4 位数字点阵（5x7），160x60 PNG，含噪点。
fn render_png(code: &str) -> Vec<u8> {
    let (w, h) = (160usize, 60usize);
    let mut img = vec![255u8; w * h]; // 白底
    let mut rng = rand::thread_rng();
    // 噪点
    for _ in 0..400 {
        let x = rng.gen_range(0..w);
        let y = rng.gen_range(0..h);
        img[y * w + x] = rng.gen_range(0..200) as u8;
    }
    // 数字（简单：用 5x7 点阵，每位间隔，缩放2x）
    for (i, ch) in code.chars().enumerate() {
        let glyph = DIGITS[ch.to_digit(10).unwrap_or(0) as usize];
        let ox = 16 + i * 36;
        let oy = 18;
        for (row, r) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (r >> (4 - col)) & 1 == 1 {
                    for dy in 0..4 {
                        for dx in 0..4 {
                            let x = ox + col * 4 + dx;
                            let y = oy + row * 4 + dy;
                            if x < w && y < h { img[y * w + x] = 20; }
                        }
                    }
                }
            }
        }
    }
    encode_grayscale_png(&img, w, h)
}

const DIGITS: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111], // 2
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110], // 3
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010], // 4
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110], // 5
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110], // 6
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000], // 7
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110], // 8
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100], // 9
];

/// 编码灰度 PNG（无压缩，单 IDAT 用 zlib stored）。
fn encode_grayscale_png(gray: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w + 1) * h);
    for y in 0..h {
        raw.push(0); // filter none
        raw.extend_from_slice(&gray[y * w..y * w + w]);
    }
    let zlib = zlib_stored(&raw);
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    chunk(&mut out, b"IHDR", &ihdr(w as u32, h as u32));
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn ihdr(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.push(8); // bit depth
    v.push(0); // grayscale
    v.push(0); v.push(0); v.push(0);
    v
}

fn chunk(out: &mut Vec<u8>, typ: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(data);
    let mut crc = 0xffffffffu32;
    for &b in typ.iter().chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 { 0xedb88320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

/// 极简 zlib：stored blocks（无压缩），适合小图。
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x78); out.push(0x01); // zlib header
    let mut i = 0;
    while i < data.len() {
        let chunk = (data.len() - i).min(65535);
        let last = i + chunk == data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(chunk as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + chunk]);
        i += chunk;
    }
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn base64_encode(data: &[u8]) -> String {
    relay_core::util::base64_encode(data)
}
