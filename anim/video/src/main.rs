//! Play a PPM frame sequence on the keyboard.
//!
//! Each LED samples the colour at its key position in the frame, so the
//! keyboard shows a coarse version of the video. Originally written to play
//! Bad Apple, now a general player for `output_<n>.ppm` sequences.

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use phantom::{Layout, Phantom, MATRIX_ENTRIES};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, ValueEnum)]
enum PlayMode {
    Full,
    Incremental,
}

#[derive(Parser)]
#[command(about = "Play a PPM frame sequence on the Tecware Phantom S")]
struct Cli {
    /// Directory containing output_1.ppm, output_2.ppm, ...
    #[arg(long)]
    frames: Option<String>,
    /// Audio file to play alongside the frames.
    #[arg(long)]
    audio: Option<String>,
    #[arg(long)]
    no_audio: bool,
    #[arg(long, default_value_t = 1)]
    start: u32,
    /// Last frame; defaults to the last existing output_<n>.ppm.
    #[arg(long)]
    end: Option<u32>,
    /// Show one frame and wait for Enter before restoring.
    #[arg(long)]
    frame: Option<u32>,
    /// Source frame rate, used for timing and audio seek.
    #[arg(long, default_value_t = 30.0)]
    fps: f64,
    /// Render every Nth frame; the time base stays at --fps.
    #[arg(long, default_value_t = 1)]
    every: u32,
    #[arg(long, value_enum, default_value_t = PlayMode::Incremental)]
    mode: PlayMode,
    #[arg(long, default_value_t = 0.001)]
    packet_delay: f64,
    #[arg(long, default_value_t = 4)]
    level: u8,
    #[arg(long, default_value = "mapping_custom.json")]
    mapping: String,
    #[arg(long, default_value = "keymap_xy.json")]
    keymap: String,
}

fn read_ppm(path: &Path) -> Result<(usize, usize, Vec<[u8; 3]>)> {
    let data = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut parts = data.splitn(4, |&b| b == b'\n');
    if parts.next() != Some(b"P6") {
        bail!("{}: not a binary PPM (P6)", path.display());
    }
    let dims = std::str::from_utf8(parts.next().unwrap_or_default())?;
    let mut it = dims.split_whitespace();
    let w: usize = it.next().unwrap_or("0").parse().unwrap_or(0);
    let h: usize = it.next().unwrap_or("0").parse().unwrap_or(0);
    let maxv = std::str::from_utf8(parts.next().unwrap_or_default())?.trim();
    if w == 0 || h == 0 || maxv != "255" {
        bail!("{}: only 8-bit PPM (maxval 255) is supported", path.display());
    }
    let pix = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("{}: truncated PPM", path.display()))?;
    if pix.len() < w * h * 3 {
        bail!("{}: truncated PPM pixel data", path.display());
    }
    Ok((
        w,
        h,
        pix.chunks_exact(3).map(|s| [s[0], s[1], s[2]]).collect(),
    ))
}

fn render(path: &Path, lay: &Layout) -> Result<Vec<u8>> {
    let (w, h, pix) = read_ppm(path)?;
    let mut m = vec![0u8; MATRIX_ENTRIES];
    for (i, p) in lay.pos.iter().enumerate() {
        let Some((nx, ny)) = p else { continue };
        let px = ((nx * (w - 1) as f64) as usize).min(w - 1);
        let py = ((ny * (h - 1) as f64) as usize).min(h - 1);
        let rgb = pix[py * w + px];
        m[i * 3] = rgb[0];
        m[i * 3 + 1] = rgb[1];
        m[i * 3 + 2] = rgb[2];
    }
    Ok(m)
}

fn start_audio(path: &str, seek: f64) -> Option<Child> {
    let mpv = Command::new("mpv")
        .args([
            "--no-video",
            "--really-quiet",
            &format!("--start={seek:.2}"),
            path,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(child) = mpv {
        return Some(child);
    }
    Command::new("ffplay")
        .args([
            "-nodisp",
            "-autoexit",
            "-loglevel",
            "quiet",
            "-ss",
            &format!("{seek:.2}"),
            path,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

fn frame_path(frames: &str, n: u32) -> PathBuf {
    Path::new(frames).join(format!("output_{n}.ppm"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let frames_dir = cli
        .frames
        .clone()
        .or_else(|| env::var("VID_FRAMES").ok())
        .context("give --frames DIR or set VID_FRAMES")?;
    let audio_path = if cli.no_audio {
        None
    } else {
        cli.audio
            .clone()
            .or_else(|| env::var("VID_AUDIO").ok())
    };

    let lay = Layout::load(&cli.mapping, &cli.keymap)?;
    println!("{} LEDs mapped", lay.count());

    if let Some(n) = cli.frame {
        let path = frame_path(&frames_dir, n);
        let m = render(&path, &lay)?;
        let d = Phantom::open()?;
        let before = d.snapshot()?;
        d.write_matrix(&m)?;
        d.enter_custom(cli.level)?;
        println!("frame {n} shown; press Enter to restore");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        d.restore(&before)?;
        return Ok(());
    }

    // Pre-render the sequence so playback timing is not affected by PPM parsing.
    let mut prerender: Vec<u8> = Vec::new();
    let mut n = 1u32;
    loop {
        let path = frame_path(&frames_dir, n);
        if !path.exists() {
            break;
        }
        if n % 200 == 0 {
            println!("prerendering frame {n}");
        }
        prerender.extend_from_slice(&render(&path, &lay)?);
        n += 1;
    }
    let available = n - 1;
    if available == 0 {
        bail!("no output_<n>.ppm frames found in {frames_dir}");
    }

    let start = cli.start.max(1);
    let end = cli.end.unwrap_or(available).min(available);
    if start > end {
        bail!("start {start} is past end {end} (only {available} frames)");
    }

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))?;
    }

    let mut audio = audio_path
        .as_deref()
        .filter(|_| !cli.no_audio)
        .and_then(|path| start_audio(path, (start.saturating_sub(1)) as f64 / cli.fps));

    let d = Phantom::open()?;
    let before = d.snapshot()?;
    d.enter_custom(cli.level)?;

    let delay = Duration::from_secs_f64(cli.packet_delay);
    let t0 = Instant::now();
    let mut prev: Option<&[u8]> = None;
    let mut played = 0u64;
    let mut n = start;

    while n <= end && !stop.load(Ordering::SeqCst) {
        if (n - start) % cli.every.max(1) != 0 {
            n += 1;
            continue;
        }
        let target = (n - start) as f64 / cli.fps;
        let now = t0.elapsed().as_secs_f64();
        if now < target {
            sleep(Duration::from_secs_f64(target - now));
        } else if now > target + 0.15 {
            // keep audio sync: skip the frames we have fallen behind on
            n = start + (now * cli.fps) as u32;
            continue;
        }

        let off = (n - 1) as usize * MATRIX_ENTRIES;
        let m = &prerender[off..off + MATRIX_ENTRIES];
        if matches!(cli.mode, PlayMode::Full) {
            d.write_matrix_fast(m, delay)?;
        } else if let Some(pv) = prev {
            d.write_delta(m, pv, delay)?;
        } else {
            d.write_matrix_fast(m, delay)?;
        }
        prev = Some(m);
        played += 1;
        n += 1;
        if played % 150 == 0 {
            println!("frame {}  {:.1}s", n - 1, t0.elapsed().as_secs_f64());
        }
    }

    if let Some(child) = &mut audio {
        let _ = child.kill();
        let _ = child.wait();
    }
    d.restore(&before)?;
    println!(
        "played {played} frames in {:.1}s",
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
