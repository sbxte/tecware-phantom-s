//! Play Bad Apple by sampling the PGM frames at each key's photo position.

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use phantom::{Layout, Phantom, MATRIX_COUNT, MATRIX_ENTRY};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

const SRC_FPS: f64 = 30.0;

#[derive(Copy, Clone, ValueEnum)]
enum PlayMode {
    Full,
    Incremental,
}

fn home() -> String {
    env::var("HOME").unwrap_or_default()
}

fn default_frames() -> String {
    env::var("BADAPPLE_FRAMES")
        .unwrap_or_else(|_| format!("{}/Programming/contrib/badapple/pgm/frames", home()))
}

fn default_audio() -> String {
    env::var("BADAPPLE_AUDIO")
        .unwrap_or_else(|_| format!("{}/Programming/contrib/badapple/badapple.mp3", home()))
}

#[derive(Parser)]
#[command(about = "Bad Apple on the Tecware Phantom S")]
struct Cli {
    #[arg(long, default_value_t = default_frames())]
    frames: String,
    #[arg(long, default_value_t = default_audio())]
    audio: String,
    #[arg(long)]
    no_audio: bool,
    #[arg(long, default_value_t = 1)]
    start: u32,
    #[arg(long, default_value_t = 6572)]
    end: u32,
    /// Show one frame and stop.
    #[arg(long)]
    frame: Option<u32>,
    #[arg(long, default_value_t = SRC_FPS)]
    fps: f64,
    /// Render every Nth frame; the time base stays at --fps.
    #[arg(long, default_value_t = 1)]
    every: u32,
    #[arg(long, default_value_t = 128)]
    threshold: u8,
    #[arg(long)]
    invert: bool,
    #[arg(long, value_enum, default_value_t = PlayMode::Incremental)]
    mode: PlayMode,
    #[arg(long, default_value_t = 0.001)]
    packet_delay: f64,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, default_value_t = 4)]
    level: u8,
    #[arg(long, default_value = "mapping_custom.json")]
    mapping: String,
    #[arg(long, default_value = "keymap_xy.json")]
    keymap: String,
}

fn read_pgm(path: &Path) -> Result<(usize, usize, Vec<u8>)> {
    let data = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut parts = data.splitn(4, |&b| b == b'\n');
    if parts.next() != Some(b"P5") {
        bail!("{}: not a binary PGM", path.display());
    }
    let dims = std::str::from_utf8(parts.next().unwrap_or_default())?;
    let mut it = dims.split_whitespace();
    let w: usize = it.next().unwrap_or("0").parse().unwrap_or(0);
    let h: usize = it.next().unwrap_or("0").parse().unwrap_or(0);
    let maxv = std::str::from_utf8(parts.next().unwrap_or_default())?.trim();
    if w == 0 || h == 0 || maxv != "255" {
        bail!("{}: bad PGM header", path.display());
    }
    let pix = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("{}: truncated PGM", path.display()))?;
    Ok((w, h, pix.to_vec()))
}

fn render(path: &Path, lay: &Layout, threshold: u8, invert: bool) -> Result<Vec<u8>> {
    let (w, h, pix) = read_pgm(path)?;
    let mut m = vec![0u8; MATRIX_COUNT * MATRIX_ENTRY];
    for (i, p) in lay.pos.iter().enumerate() {
        let Some((nx, ny)) = p else { continue };
        let px = ((nx * (w - 1) as f64) as usize).min(w - 1);
        let py = ((ny * (h - 1) as f64) as usize).min(h - 1);
        let mut lit = pix[py * w + px] >= threshold;
        if invert {
            lit = !lit;
        }
        if lit {
            m[i * 3..i * 3 + 3].fill(255);
        }
    }
    Ok(m)
}

fn lit_count(m: &[u8]) -> usize {
    m.chunks_exact(3).filter(|c| c.iter().any(|b| *b != 0)).count()
}

fn start_audio(path: &str, seek: f64) -> Option<Child> {
    let mpv = Command::new("mpv")
        .args(["--no-video", "--really-quiet", &format!("--start={seek:.2}"), path])
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
    Path::new(frames).join(format!("output_{n:04}.pgm"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let lay = Layout::load(&cli.mapping, &cli.keymap)?;
    println!("{} LEDs mapped", lay.count());

    if let Some(n) = cli.frame {
        let path = frame_path(&cli.frames, n);
        let m = render(&path, &lay, cli.threshold, cli.invert)?;
        if cli.dry_run {
            println!("dry run: {} lit LEDs for {}", lit_count(&m), path.display());
            return Ok(());
        }
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

    let first = frame_path(&cli.frames, cli.start);
    if !first.exists() {
        bail!("frame not found: {}", first.display());
    }

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))?;
    }

    let mut audio = if cli.no_audio || cli.dry_run {
        None
    } else {
        start_audio(&cli.audio, (cli.start.saturating_sub(1)) as f64 / SRC_FPS)
    };

    let mut d = if cli.dry_run {
        None
    } else {
        Some(Phantom::open()?)
    };

    let before = match &d {
        Some(dev) => Some(dev.snapshot()?),
        None => None,
    };
    if let Some(dev) = &d {
        dev.enter_custom(cli.level)?;
    }

    let delay = Duration::from_secs_f64(cli.packet_delay);
    let t0 = Instant::now();
    let mut prev: Option<Vec<u8>> = None;
    let mut played = 0u64;
    let mut n = cli.start;

    while n <= cli.end && !stop.load(Ordering::SeqCst) {
        if (n - cli.start) % cli.every.max(1) != 0 {
            n += 1;
            continue;
        }
        let target = (n - cli.start) as f64 / cli.fps;
        let now = t0.elapsed().as_secs_f64();
        if now < target {
            sleep(Duration::from_secs_f64(target - now));
        } else if now > target + 0.15 {
            // keep audio sync: skip the frames we have fallen behind on
            n = cli.start + (now * cli.fps) as u32;
            continue;
        }

        let path = frame_path(&cli.frames, n);
        if !path.exists() {
            n += 1;
            continue;
        }
        let m = render(&path, &lay, cli.threshold, cli.invert)?;
        if cli.dry_run {
            if n % 100 == 0 {
                println!("dry {n} lit={}", lit_count(&m));
            }
        } else if let Some(dev) = &mut d {
            if matches!(cli.mode, PlayMode::Full) {
                dev.write_matrix_fast(&m, delay)?;
            } else if let Some(pv) = &prev {
                dev.write_delta(&m, pv, delay)?;
            } else {
                dev.write_matrix_fast(&m, delay)?;
            }
            prev = Some(m);
        }
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
    if let (Some(dev), Some(before)) = (&d, &before) {
        dev.restore(before)?;
    }
    println!(
        "played {played} frames in {:.1}s",
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
