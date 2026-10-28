//! Core protocol/device library for the Tecware Phantom S (VID 379a, PID 0920).
//!
//! Pumped by the `phantom` CLI and the animation binaries in `anim/`.

use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use hidapi::{HidApi, HidDevice};
use std::fs;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const VID: u16 = 0x379A;
pub const PID: u16 = 0x0920;
pub const REPORT_ID: u8 = 0x06;
pub const PACKET: usize = 64;
pub const HEADER: usize = 7; // cmd, addr lo/hi, len, csum lo/hi, result
pub const CAPACITY: usize = PACKET - 1 - HEADER;
pub const AREA: usize = 128;
pub const MATRIX_COUNT: usize = 392;      // custom-light LEDs
pub const MATRIX_ENTRY: usize = 3;        // RGB888 per LED
pub const KEYMAP_ENTRIES: usize = 128;
pub const FNLOCK_SIZE: usize = 256;
pub const MACRO_SIZE: usize = 2048;
pub const RESULT_OK: u8 = 0x55;

pub const SCREEN_OPEN: u8 = 0x40;
pub const SCREEN_DATA: u8 = 0x41;
pub const SCREEN_FINISH: u8 = 0x42;
pub const PKT_SET_TIME: u8 = 9;
pub const PKT_SET_DATE: u8 = 10;
pub const PKT_HOME: u8 = 11;
pub const PKT_IMAGE_DATA: u8 = 12;
pub const PKT_SWITCH_IMAGE: u8 = 13;
pub const PKT_SWITCH_GIF: u8 = 15;
pub const PKT_GUI_EVENT: u8 = 16;
pub const PKT_GIF_DATA: u8 = 17;
pub const PKT_GIF_FRAMES: u8 = 18;
pub const PKT_GIF_FPS: u8 = 19;
pub const GUI_IMAGE_UPLOAD: u8 = 1;
pub const GUI_GIF_FRAME: u8 = 2;

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Mode {
    Wave,
    Cloud,
    Neon,
    Vortex,
    Flower,
    Fly,
    Flow,
    Rain,
    Mixcolor,
    Breathe,
    Static,
    Ripple,
    Click,
    Reaction,
    Speed,
    Scan,
    Change,
    Custom,
    Off,
}

impl Mode {
    pub fn id(self) -> u8 {
        use Mode::*;
        match self {
            Wave => 0,
            Cloud => 1,
            Neon => 2,
            Vortex => 3,
            Flower => 4,
            Fly => 5,
            Flow => 6,
            Rain => 7,
            Mixcolor => 8,
            Breathe => 9,
            Static => 10,
            Ripple => 11,
            Click => 12,
            Reaction => 13,
            Speed => 14,
            Scan => 15,
            Change => 16,
            Custom => 17,
            Off => 18,
        }
    }

    pub fn name(id: u8) -> &'static str {
        use Mode::*;
        [
            Wave, Cloud, Neon, Vortex, Flower, Fly, Flow, Rain, Mixcolor, Breathe, Static,
            Ripple, Click, Reaction, Speed, Scan, Change, Custom, Off,
        ]
        .into_iter()
        .find(|m| m.id() == id)
        .map(|m| m.into_name())
        .unwrap_or("?")
    }

    pub fn into_name(self) -> &'static str {
        use Mode::*;
        match self {
            Wave => "wave",
            Cloud => "cloud",
            Neon => "neon",
            Vortex => "vortex",
            Flower => "flower",
            Fly => "fly",
            Flow => "flow",
            Rain => "rain",
            Mixcolor => "mixcolor",
            Breathe => "breathe",
            Static => "static",
            Ripple => "ripple",
            Click => "click",
            Reaction => "reaction",
            Speed => "speed",
            Scan => "scan",
            Change => "change",
            Custom => "custom",
            Off => "off",
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub enum Direction {
    #[value(name = "left-to-right")]
    LeftToRight,
    #[value(name = "right-to-left")]
    RightToLeft,
    #[value(name = "top-to-bottom")]
    TopToBottom,
    #[value(name = "bottom-to-top")]
    BottomToTop,
    #[value(name = "ccw")]
    Ccw,
    #[value(name = "cw")]
    Cw,
    #[value(name = "in-to-out")]
    InToOut,
    #[value(name = "out-to-in")]
    OutToIn,
}

impl Direction {
    pub fn id(self) -> u8 {
        use Direction::*;
        match self {
            LeftToRight => 0,
            RightToLeft => 1,
            TopToBottom => 2,
            BottomToTop => 3,
            Ccw => 4,
            Cw => 5,
            InToOut => 6,
            OutToIn => 7,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub enum Layer {
    Win,
    #[value(name = "win-fn")]
    Winfn,
}

impl Layer {
    pub fn cmds(self) -> (u8, u8, u8) {
        match self {
            Layer::Win => (0x16, 0x17, 0x18),
            Layer::Winfn => (0x32, 0x33, 0x34),
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub enum Page {
    Home,
    Image,
    Gif,
    Time,
}

pub fn parse_color(s: &str) -> Result<(u8, u8, u8)> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        bail!("colour must be RRGGBB");
    }
    Ok((
        u8::from_str_radix(&s[0..2], 16)?,
        u8::from_str_radix(&s[2..4], 16)?,
        u8::from_str_radix(&s[4..6], 16)?,
    ))
}

pub fn parse_key(s: &str) -> Result<(usize, (u8, u8, u8))> {
    let (idx, col) = s
        .split_once('=')
        .ok_or_else(|| anyhow!("expected N=RRGGBB"))?;
    let idx = if let Some(h) = idx.strip_prefix("0x") {
        usize::from_str_radix(h, 16)?
    } else {
        idx.parse()?
    };
    Ok((idx, parse_color(col)?))
}

pub struct Layout {
    pub pos: Vec<Option<(f64, f64)>>,   // normalised to the key bounding box
}

impl Layout {
    pub fn load(mapping: &str, keymap: &str) -> Result<Self> {
        let m: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(mapping).with_context(|| format!("read {mapping}"))?)?;
        let k: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(keymap).with_context(|| format!("read {keymap}"))?)?;
        let keys = k
            .get("keys")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow!("{keymap}: no \"keys\" object"))?;

        let mut raw: Vec<(usize, f64, f64)> = Vec::new();
        if let Some(obj) = m.as_object() {
            for (idx_s, v) in obj {
                let idx: usize = match idx_s.parse() {
                    Ok(i) => i,
                    Err(_) => continue,
                };
                if idx >= MATRIX_COUNT {
                    continue;
                }
                let name = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Array(a) => match a.first() {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        _ => continue,
                    },
                    _ => continue,
                };
                let lower = name.to_lowercase();
                if lower.contains("red somehow") || lower.contains("faint red") || name == "[]" {
                    continue;
                }
                if let Some(xy) = keys.get(&name).and_then(|v| v.as_array()) {
                    if xy.len() >= 2 {
                        raw.push((idx, xy[0].as_f64().unwrap_or(0.0), xy[1].as_f64().unwrap_or(0.0)));
                    }
                }
            }
        }
        if raw.is_empty() {
            bail!("no LEDs matched {keymap}");
        }
        let (mut minx, mut maxx) = (f64::MAX, f64::MIN);
        let (mut miny, mut maxy) = (f64::MAX, f64::MIN);
        for (_, x, y) in &raw {
            minx = minx.min(*x); maxx = maxx.max(*x);
            miny = miny.min(*y); maxy = maxy.max(*y);
        }
        let sx = (maxx - minx).max(1e-9);
        let sy = (maxy - miny).max(1e-9);
        let mut pos = vec![None; MATRIX_COUNT];
        for (idx, x, y) in raw {
            pos[idx] = Some(((x - minx) / sx, (y - miny) / sy));
        }
        Ok(Self { pos })
    }

    pub fn count(&self) -> usize {
        self.pos.iter().filter(|p| p.is_some()).count()
    }
}

/// Full restorable device state: function area + custom matrix.
#[derive(Clone)]
pub struct State {
    pub area: Vec<u8>,
    pub matrix: Vec<u8>,
}

pub struct Phantom {
    dev: HidDevice,
}

impl Phantom {
    pub fn open() -> Result<Self> {
        let api = HidApi::new().context("hidapi init")?;
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID && d.interface_number() == 1)
            .ok_or_else(|| anyhow!("device not found (VID 379a PID 0920 interface 1)"))?;
        let dev = api.open_path(info.path()).context("open device")?;
        Ok(Self { dev })
    }

    pub fn packet(cmd: u8, data: &[u8], addr: u16) -> Vec<u8> {
        let mut b = vec![0u8; PACKET];
        b[0] = REPORT_ID;
        b[1] = cmd;
        b[2] = (addr & 0xff) as u8;
        b[3] = (addr >> 8) as u8;
        b[4] = data.len() as u8;
        b[8..8 + data.len()].copy_from_slice(data);
        let sum: u16 = b[1..5]
            .iter()
            .chain(b[7..].iter())
            .map(|&x| x as u16)
            .sum();
        b[5] = (sum & 0xff) as u8;
        b[6] = (sum >> 8) as u8;
        b
    }

    pub fn exchange(&self, cmd: u8, data: &[u8], addr: u16, timeout_ms: i32) -> Result<Vec<u8>> {
        for _ in 0..3 {
            self.dev.write(&Self::packet(cmd, data, addr))?;
            let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
            while Instant::now() < deadline {
                let mut buf = [0u8; 256];
                let n = self.dev.read_timeout(&mut buf, 200)?;
                if n < 8 || buf[0] != REPORT_ID || buf[1] != cmd {
                    continue;
                }
                if buf[7] != RESULT_OK {
                    bail!(
                        "device rejected cmd {cmd:#04x} addr {addr} (result {:#04x})",
                        buf[7]
                    );
                }
                let len = buf[4] as usize;
                return Ok(buf[8..8 + len].to_vec());
            }
        }
        bail!("no reply for cmd {cmd:#04x} addr {addr}")
    }

    pub fn read_area(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(AREA);
        let mut addr = 0usize;
        while addr < AREA {
            let n = CAPACITY.min(AREA - addr);
            out.extend(self.exchange(0x14, &vec![0u8; n], addr as u16, 600)?);
            addr += n;
        }
        Ok(out)
    }

    pub fn write_area(&self, area: &[u8]) -> Result<()> {
        if area.len() != AREA {
            bail!("function area must be {AREA} bytes");
        }
        for addr in (0..AREA).step_by(CAPACITY) {
            let end = (addr + CAPACITY).min(AREA);
            self.exchange(0x15, &area[addr..end], addr as u16, 600)?;
        }
        Ok(())
    }

    pub fn set_light(
        &self,
        mode: Option<u8>,
        level: Option<u8>,
        speed: Option<u8>,
        color: Option<(u8, u8, u8)>,
        rainbow: bool,
        direction: Option<u8>,
    ) -> Result<Vec<u8>> {
        let mut a = self.read_area()?;
        if let Some(m) = mode {
            a[1] = m;
        }
        if let Some(l) = level {
            a[2] = l.min(4);
        }
        if let Some(s) = speed {
            a[3] = s.min(4);
        }
        if let Some(d) = direction {
            a[4] = d & 0x07;
        }
        if rainbow {
            a[5] = 1;             // built-in 7-colour cycle (preset effects only)
        }
        if let Some((r, g, b)) = color {
            a[5] = 0;             // single colour
            a[6] = r;
            a[7] = g;
            a[8] = b;
        }
        self.write_area(&a)?;
        Ok(a)
    }

    pub fn device_info(&self) -> Result<Vec<u8>> {
        let mut out = self.exchange(0x12, &[0u8; CAPACITY], 0, 600)?;
        out.extend(self.exchange(0x12, &[0u8; 1], CAPACITY as u16, 600)?);
        Ok(out)
    }

    pub fn read_blob(&self, cmd: u8, total: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(total);
        let mut addr = 0usize;
        while addr < total {
            let n = CAPACITY.min(total - addr);
            out.extend(self.exchange(cmd, &vec![0u8; n], addr as u16, 800)?);
            addr += n;
        }
        Ok(out)
    }

    pub fn write_blob(&self, cmd: u8, blob: &[u8]) -> Result<()> {
        let mut addr = 0usize;
        while addr < blob.len() {
            let end = (addr + CAPACITY).min(blob.len());
            self.exchange(cmd, &blob[addr..end], addr as u16, 800)?;
            addr += CAPACITY;
        }
        Ok(())
    }

    pub fn read_matrix(&self) -> Result<Vec<u8>> {
        self.read_blob(0x19, MATRIX_COUNT * MATRIX_ENTRY)
    }

    pub fn write_matrix(&self, m: &[u8]) -> Result<()> {
        if m.len() != MATRIX_COUNT * MATRIX_ENTRY {
            bail!("matrix must be {} bytes", MATRIX_COUNT * MATRIX_ENTRY);
        }
        self.write_blob(0x1A, m)
    }

    /// Send only changed LEDs; contiguous runs are merged into one packet.
    pub fn write_delta(&self, m: &[u8], prev: &[u8], delay: Duration) -> Result<()> {
        let mut addr = 0usize;
        while addr < m.len() {
            let end = (addr + MATRIX_ENTRY).min(m.len());
            if m[addr..end] == prev[addr..end] {
                addr = end;
                continue;
            }
            let run_start = addr;
            let mut run_end = end;
            while run_end < m.len() {
                let e = (run_end + MATRIX_ENTRY).min(m.len());
                if m[run_end..e] != prev[run_end..e]
                    && run_end - run_start + MATRIX_ENTRY <= (CAPACITY / MATRIX_ENTRY) * MATRIX_ENTRY
                {
                    run_end = e;
                } else {
                    break;
                }
            }
            self.dev.write(&Self::packet(0x1A, &m[run_start..run_end], run_start as u16))?;
            if !delay.is_zero() {
                sleep(delay);
            }
            addr = run_end;
        }
        Ok(())
    }

    pub fn crc16(data: &[u8]) -> u16 {
        let mut crc: u16 = 0xFFFF;
        for &b in data {
            crc ^= b as u16;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
            }
        }
        crc
    }

    pub fn screen_packet(pkt: u8, data: &[u8], declared: Option<usize>) -> Vec<u8> {
        let n = declared.unwrap_or(data.len()) as u16;
        let head = [pkt, (n >> 8) as u8, (n & 0xff) as u8];
        let crc = Self::crc16(&head);
        let mut out = vec![0xA5, 0x5A, head[0], head[1], head[2], (crc >> 8) as u8, crc as u8];
        out.extend_from_slice(data);
        out
    }

    pub fn screen_send(&self, opened: &mut bool, payload: &[u8], repeat: Option<&[u8]>) -> Result<()> {
        let cap = CAPACITY - repeat.map_or(0, |r| r.len());
        let mut addr = 0usize;
        while addr < payload.len() {
            let end = (addr + cap).min(payload.len());
            let mut data = Vec::with_capacity(cap + repeat.map_or(0, |r| r.len()));
            if let Some(r) = repeat {
                data.extend_from_slice(r);
            }
            data.extend_from_slice(&payload[addr..end]);
            let cmd = if *opened { SCREEN_DATA } else { SCREEN_OPEN };
            self.exchange(cmd, &data, addr as u16, 3000)?;
            *opened = true;
            addr += cap;
        }
        Ok(())
    }

    pub fn screen_finish(&self) -> Result<()> {
        self.exchange(SCREEN_FINISH, &vec![0u8; CAPACITY], 0, 6000)?;
        Ok(())
    }

    pub fn screen_simple(&self, pkt: u8, data: &[u8]) -> Result<()> {
        let mut opened = false;
        self.screen_send(&mut opened, &Self::screen_packet(pkt, data, None), None)?;
        self.screen_finish()
    }

    pub fn upload_image(&self, pixels: &[u8], w: u32, h: u32) -> Result<()> {
        let mut opened = false;
        self.screen_send(
            &mut opened,
            &Self::screen_packet(PKT_GUI_EVENT, &[GUI_IMAGE_UPLOAD], None),
            None,
        )?;
        sleep(Duration::from_millis(350));
        self.screen_send(
            &mut opened,
            &Self::screen_packet(PKT_IMAGE_DATA, &[], Some(pixels.len())),
            None,
        )?;
        sleep(Duration::from_millis(50));
        self.screen_send(&mut opened, pixels, None)?;
        sleep(Duration::from_millis(500));
        self.screen_finish()?;
        sleep(Duration::from_millis(300));
        let _ = (w, h);
        Ok(())
    }

    pub fn upload_gif(&self, frames: &[Vec<u8>], slot: u8, fps: u8) -> Result<()> {
        if frames.is_empty() {
            bail!("no frames");
        }
        let frame_len = frames[0].len();
        let hdr = Self::screen_packet(PKT_GIF_DATA, &[], Some(frame_len));
        let mut opened = false;
        self.screen_send(
            &mut opened,
            &Self::screen_packet(PKT_GIF_FRAMES, &[slot, 0], None),
            None,
        )?;
        self.screen_send(
            &mut opened,
            &Self::screen_packet(PKT_GIF_FPS, &[slot, 0], None),
            None,
        )?;
        sleep(Duration::from_millis(30));
        for (i, f) in frames.iter().enumerate() {
            self.screen_send(
                &mut opened,
                &Self::screen_packet(PKT_GUI_EVENT, &[GUI_GIF_FRAME, slot, i as u8], None),
                None,
            )?;
            sleep(Duration::from_millis(if i % 16 == 0 { 3000 } else { 30 }));
            self.screen_send(&mut opened, &hdr, None)?;
            for chunk in f.chunks(1024) {
                self.screen_send(&mut opened, chunk, Some(&hdr))?;
                if f.len() > 1024 {
                    sleep(Duration::from_millis(30));
                }
            }
        }
        self.screen_send(
            &mut opened,
            &Self::screen_packet(PKT_GIF_FRAMES, &[slot, frames.len() as u8], None),
            None,
        )?;
        sleep(Duration::from_millis(30));
        self.screen_send(
            &mut opened,
            &Self::screen_packet(PKT_GIF_FPS, &[slot, fps], None),
            None,
        )?;
        sleep(Duration::from_millis(30));
        self.screen_finish()
    }

    pub fn sync_clock(&self) -> Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let tod = (secs % 86400) as u32;
        let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
        let days = secs / 86400;
        let (y, mo, d, wd) = civil_from_days(days as i64);
        self.screen_simple(PKT_SET_TIME, &[h as u8, m as u8, s as u8])?;
        self.screen_simple(PKT_SET_DATE, &[(y % 100) as u8, wd, mo as u8, d as u8])?;
        Ok(())
    }
}



impl Phantom {
    /// Write the whole matrix without waiting for replies (animation path).
    pub fn write_matrix_fast(&self, m: &[u8], delay: Duration) -> Result<()> {
        if m.len() != MATRIX_COUNT * MATRIX_ENTRY {
            bail!("matrix must be {} bytes", MATRIX_COUNT * MATRIX_ENTRY);
        }
        let mut addr = 0usize;
        while addr < m.len() {
            let end = (addr + CAPACITY).min(m.len());
            self.dev
                .write(&Self::packet(0x1A, &m[addr..end], addr as u16))?;
            if !delay.is_zero() {
                sleep(delay);
            }
            addr += CAPACITY;
        }
        Ok(())
    }

    /// Switch to custom mode at `level`.
    pub fn enter_custom(&self, level: u8) -> Result<()> {
        let mut a = self.read_area()?;
        a[1] = 17;
        a[2] = level.min(4);
        a[5] = 0;
        self.write_area(&a)
    }

    /// Capture everything an animation can disturb.
    pub fn snapshot(&self) -> Result<State> {
        Ok(State {
            area: self.read_area()?,
            matrix: self.read_matrix()?,
        })
    }

    /// Put the device back exactly as it was before `snapshot`.
    pub fn restore(&self, s: &State) -> Result<()> {
        self.write_area(&s.area)?;
        self.write_matrix(&s.matrix)
    }
}

fn civil_from_days(z: i64) -> (i64, u32, u32, u8) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    let wd = ((z + 3) % 7 + 1) as u8; // 1970-01-01 was Thursday
    (y, m, d, wd)
}

fn to_rgb565(img: &image::RgbImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.pixels().len() * 2);
    for p in img.pixels() {
        let [r, g, b] = p.0;
        let v = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
        out.push((v >> 8) as u8);
        out.push(v as u8);
    }
    out
}

pub fn load_image(path: &str, w: u32, h: u32) -> Result<Vec<u8>> {
    let img = image::open(path).with_context(|| format!("open {path}"))?;
    let img = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
    Ok(to_rgb565(&img.to_rgb8()))
}

pub fn load_gif(path: &str, w: u32, h: u32) -> Result<Vec<Vec<u8>>> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let file = fs::File::open(path)?;
    let mut decoder = opts.read_info(file)?;
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame()? {
        let rgba = image::RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.buffer.to_vec())
            .ok_or_else(|| anyhow!("bad gif frame"))?;
        let resized = image::imageops::resize(&rgba, w, h, image::imageops::FilterType::Lanczos3);
        frames.push(to_rgb565(&image::DynamicImage::ImageRgba8(resized).to_rgb8()));
    }
    Ok(frames)
}

pub fn hsv(h: f64) -> (u8, u8, u8) {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f64;
    let q = 1.0 - f;
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (1.0, f, 0.0),
        1 => (q, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, q, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
