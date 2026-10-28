//! Slow left-to-right rainbow sweep across the physical key positions.

use anyhow::Result;
use clap::Parser;
use phantom::{hsv, Layout, Phantom, MATRIX_COUNT, MATRIX_ENTRY};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(about = "Rainbow band sweeping left to right")]
struct Cli {
    #[arg(long, default_value_t = 300.0)]
    seconds: f64,
    #[arg(long, default_value_t = 14.0, help = "seconds per sweep")]
    period: f64,
    #[arg(long, default_value_t = 2.0, help = "black gap between sweeps")]
    pause: f64,
    #[arg(long, default_value_t = 0.15, help = "band width in x fraction")]
    width: f64,
    #[arg(long, default_value_t = 25.0)]
    fps: f64,
    #[arg(long, default_value_t = 0.002, help = "delay after each LED packet")]
    packet_delay: f64,
    #[arg(long)]
    once: bool,
    #[arg(long, default_value = "mapping_custom.json")]
    mapping: String,
    #[arg(long, default_value = "keymap_xy.json")]
    keymap: String,
    #[arg(long, default_value_t = 4)]
    level: u8,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let lay = Layout::load(&cli.mapping, &cli.keymap)?;
    println!("positions: {}/{}", lay.count(), MATRIX_COUNT);

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))?;
    }

    let d = Phantom::open()?;
    let before = d.snapshot()?;

    let xs: Vec<Option<f64>> = lay.pos.iter().map(|p| p.map(|(x, _)| x)).collect();
    let len = MATRIX_COUNT * MATRIX_ENTRY;
    let delay = Duration::from_secs_f64(cli.packet_delay);

    // Start from black so the first delta is predictable.
    d.write_matrix(&vec![0u8; len])?;
    d.enter_custom(cli.level)?;

    let mut frame = vec![0u8; len];
    let mut prev = vec![0u8; len];
    let start_all = Instant::now();
    let deadline = Duration::from_secs_f64(cli.seconds);

    'outer: loop {
        let cycle = Instant::now();
        loop {
            if stop.load(Ordering::SeqCst) {
                break 'outer;
            }
            let el = cycle.elapsed().as_secs_f64();
            if el >= cli.period + cli.pause {
                break;
            }
            let p = if el < cli.period {
                -cli.width / 2.0 + (1.0 + cli.width) * (el / cli.period)
            } else {
                2.0
            };
            for (i, x) in xs.iter().enumerate() {
                let rgb = match x {
                    Some(x) if (x - p).abs() <= cli.width / 2.0 => {
                        hsv(((x - (p - cli.width / 2.0)) / cli.width).rem_euclid(1.0))
                    }
                    _ => (0, 0, 0),
                };
                frame[i * 3..i * 3 + 3].copy_from_slice(&[rgb.0, rgb.1, rgb.2]);
            }
            d.write_delta(&frame, &prev, delay)?;
            prev.copy_from_slice(&frame);
            sleep(Duration::from_secs_f64(1.0 / cli.fps));
        }
        if cli.once || start_all.elapsed() >= deadline {
            break;
        }
    }

    d.restore(&before)?;
    println!("done, previous state restored");
    Ok(())
}
