use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use phantom::{
    load_gif, load_image, parse_color, parse_key, Direction, Layer, Mode, Page, Phantom,
    FNLOCK_SIZE, KEYMAP_ENTRIES, MACRO_SIZE, MATRIX_COUNT, MATRIX_ENTRY, PKT_HOME,
    PKT_SWITCH_GIF, PKT_SWITCH_IMAGE,
};
use std::fs;

#[derive(Parser)]
#[command(about = "Tecware Phantom S lighting control")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show current lighting state
    Status,
    /// Set effect, brightness, speed and colour
    Light {
        #[arg(long, value_enum)]
        mode: Option<Mode>,
        #[arg(long)]
        level: Option<u8>,
        #[arg(long)]
        speed: Option<u8>,
        #[arg(long, value_parser = parse_color)]
        color: Option<(u8, u8, u8)>,
        /// Use the built-in 7-colour cycle (preset effects only)
        #[arg(long)]
        rainbow: bool,
        /// Effect direction (trend field)
        #[arg(long, value_enum)]
        direction: Option<Direction>,
    },
    /// Turn the backlight off
    Off,
    /// Set the USB LED auto-off timer in seconds
    Timer { seconds: u16 },
    /// Per-key custom colours (mode custom)
    Perkey {
        #[arg(long, value_parser = parse_color)]
        all: Option<(u8, u8, u8)>,
        #[arg(long = "set", value_parser = parse_key)]
        keys: Vec<(usize, (u8, u8, u8))>,
        #[arg(long)]
        load: Option<String>,
        #[arg(long)]
        save: Option<String>,
        #[arg(long)]
        no_mode: bool,
        #[arg(long)]
        level: Option<u8>,
    },
    /// Read/write a keymap layer (3 bytes per key, 128 entries)
    Keymap {
        #[arg(long, value_enum, default_value_t = Layer::Win)]
        layer: Layer,
        #[arg(long)]
        save: Option<String>,
        #[arg(long)]
        load: Option<String>,
    },
    /// Read the FN-lock list (256 bytes)
    Fnlock {
        #[arg(long)]
        save: Option<String>,
    },
    /// Read/write the macro region (2048 bytes)
    Macro {
        #[arg(long)]
        save: Option<String>,
        #[arg(long)]
        load: Option<String>,
    },
    /// Battery percentage and charging flag
    Battery,
    /// Read or set the onboard profile index
    Profile {
        #[arg(long)]
        set: Option<u8>,
    },
    /// Upload a still image to the TFT
    Image {
        file: String,
        #[arg(long, default_value_t = 128)]
        width: u32,
        #[arg(long, default_value_t = 128)]
        height: u32,
        #[arg(long)]
        no_switch: bool,
    },
    /// Upload an animated GIF to the TFT
    Gif {
        file: String,
        #[arg(long, default_value = "user")]
        slot: String,
        #[arg(long, default_value_t = 12)]
        fps: u8,
        #[arg(long, default_value_t = 128)]
        width: u32,
        #[arg(long, default_value_t = 128)]
        height: u32,
    },
    /// Switch TFT page or sync the clock
    Screen {
        #[arg(value_enum)]
        page: Page,
    },
}

fn main() -> Result<()> {
    // behave like a normal unix tool when piped into head/less
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    let d = Phantom::open()?;
    match cli.cmd {
        Cmd::Status => {
            let a = d.read_area()?;
            println!("mode         : {} ({})", a[1], Mode::name(a[1]));
            println!("brightness   : {}", a[2]);
            println!("speed        : {}", a[3]);
            println!("direction    : {}", a[4]);
            println!("use colour   : {}", a[5]);
            println!("colour       : {:02x}{:02x}{:02x}", a[6], a[7], a[8]);
            println!("usb led timer: {}", u16::from_le_bytes([a[23], a[24]]));
            let info = d.device_info()?;
            if info.len() >= 51 {
                println!(
                    "keymap count : {}   light entry: {}   light count: {}",
                    info[4],
                    info[48],
                    u16::from_le_bytes([info[49], info[50]])
                );
            }
        }
        Cmd::Light {
            mode,
            level,
            speed,
            color,
            rainbow,
            direction,
        } => {
            let a = d.set_light(mode.map(|m| m.id()), level, speed, color, rainbow,
                        direction.map(|d| d.id()))?;
            println!(
                "mode={} ({}) level={} speed={} direction={} colour={:02x}{:02x}{:02x} rgb={}",
                a[1],
                Mode::name(a[1]),
                a[2],
                a[3],
                a[4],
                a[6],
                a[7],
                a[8],
                a[5]
            );
        }
        Cmd::Off => {
            d.set_light(Some(Mode::Off.id()), None, None, None, false, None)?;
            println!("backlight off");
        }
        Cmd::Timer { seconds } => {
            let mut a = d.read_area()?;
            a[23..25].copy_from_slice(&seconds.to_le_bytes());
            d.write_area(&a)?;
            println!("usb led auto-off timer = {seconds}s");
        }
        Cmd::Perkey {
            all,
            keys,
            load,
            save,
            no_mode,
            level,
        } => {
            let mut m = if let Some(p) = &load {
                fs::read(&p).with_context(|| format!("read {p}"))?
            } else {
                d.read_matrix()?
            };
            if m.len() != MATRIX_COUNT * MATRIX_ENTRY {
                bail!("matrix file must be {} bytes", MATRIX_COUNT * MATRIX_ENTRY);
            }
            if let Some((r, g, b)) = all {
                for i in 0..MATRIX_COUNT {
                    m[i * 3..i * 3 + 3].copy_from_slice(&[r, g, b]);
                }
            }
            for (idx, (r, g, b)) in keys {
                if idx >= MATRIX_COUNT {
                    bail!("key index {idx} out of range 0..{}", MATRIX_COUNT - 1);
                }
                m[idx * 3..idx * 3 + 3].copy_from_slice(&[r, g, b]);
            }
            d.write_matrix(&m)?;
            if let Some(p) = save {
                fs::write(&p, &m)?;
                println!("saved matrix to {p}");
            }
            if !no_mode {
                d.set_light(Some(Mode::Custom.id()), level, None, None, false, None)?;
            }
            println!("custom matrix written");
        }
        Cmd::Keymap { layer, save, load } => {
            let (default, read, write) = layer.cmds();
            let len = KEYMAP_ENTRIES * MATRIX_ENTRY;
            if let Some(p) = load {
                let blob = fs::read(&p).with_context(|| format!("read {p}"))?;
                if blob.len() != len {
                    bail!("keymap file must be {len} bytes");
                }
                d.write_blob(write, &blob)?;
                println!("wrote {len} bytes to keymap");
            } else if let Some(p) = save {
                let blob = d.read_blob(read, len)?;
                fs::write(&p, &blob)?;
                println!("saved keymap to {p}");
            } else {
                let cur = d.read_blob(read, len)?;
                let def = d.read_blob(default, len)?;
                println!("layer read=0x{read:02x} write=0x{write:02x} entries={KEYMAP_ENTRIES}");
                println!("current == default: {}", cur == def);
                for i in 0..KEYMAP_ENTRIES.min(12) {
                    let e = &cur[i * 3..i * 3 + 3];
                    println!("  key {i:3}: type={:#04x} low={:#04x} high={:#04x}", e[0], e[1], e[2]);
                }
            }
        }
        Cmd::Fnlock { save } => {
            let blob = d.read_blob(0x35, FNLOCK_SIZE)?;
            if let Some(p) = save {
                fs::write(&p, &blob)?;
                println!("saved fn-lock list to {p}");
            } else {
                let nz = blob.iter().filter(|b| **b != 0).count();
                println!("fn-lock list: {nz}/{FNLOCK_SIZE} non-zero");
                println!("first 32: {}", blob[..32].iter().map(|b| format!("{b:02x}")).collect::<String>());
            }
        }
        Cmd::Macro { save, load } => {
            if let Some(p) = load {
                let blob = fs::read(&p).with_context(|| format!("read {p}"))?;
                if blob.len() != MACRO_SIZE {
                    bail!("macro file must be {MACRO_SIZE} bytes");
                }
                d.write_blob(0x1C, &blob)?;
                println!("wrote {MACRO_SIZE} bytes of macros");
            } else {
                let blob = d.read_blob(0x1B, MACRO_SIZE)?;
                let nz = blob.iter().filter(|b| **b != 0).count();
                if let Some(p) = save {
                    fs::write(&p, &blob)?;
                    println!("saved macro region to {p} ({nz}/{MACRO_SIZE} non-zero)");
                } else {
                    println!("macro region: {nz}/{MACRO_SIZE} non-zero");
                }
            }
        }
        Cmd::Battery => {
            let b = d.read_blob(0x1F, 2)?;
            let pct = b.first().copied().unwrap_or(0);
            let flag = b.get(1).copied().unwrap_or(0);
            println!("battery: {pct}%  charging flag: {flag}");
        }
        Cmd::Profile { set } => {
            if let Some(n) = set {
                d.write_blob(0x31, &[n])?;
                println!("profile set to {n}");
            } else {
                let b = d.read_blob(0x30, 1)?;
                println!("profile index: {}", b.first().copied().unwrap_or(0));
            }
        }
        Cmd::Image {
            file,
            width,
            height,
            no_switch,
        } => {
            let px = load_image(&file, width, height)?;
            d.upload_image(&px, width, height)?;
            if !no_switch {
                d.screen_simple(PKT_SWITCH_IMAGE, &[])?;
            }
            println!("uploaded {width}x{height} image");
        }
        Cmd::Gif {
            file,
            slot,
            fps,
            width,
            height,
        } => {
            let slot_code = match slot.as_str() {
                "boot" => 0,
                "user" => 1,
                "home" => 2,
                _ => bail!("slot must be boot, user or home"),
            };
            let frames = load_gif(&file, width, height)?;
            d.upload_gif(&frames, slot_code, fps)?;
            d.screen_simple(PKT_SWITCH_GIF, &[])?;
            println!("uploaded {} frames at {fps} fps", frames.len());
        }
        Cmd::Screen { page } => {
            match page {
                Page::Home => d.screen_simple(PKT_HOME, &[])?,
                Page::Image => d.screen_simple(PKT_SWITCH_IMAGE, &[])?,
                Page::Gif => d.screen_simple(PKT_SWITCH_GIF, &[])?,
                Page::Time => d.sync_clock()?,
            }
            println!("ok");
        }
    }
    Ok(())
}
