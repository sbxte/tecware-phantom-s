# Tecware Phantom S on Linux

Reverse-engineered control and documentation for the Tecware Phantom S
(`379a:0920`, I-CHIP `KF010`) keyboard: a native Rust CLI, animation binaries,
and a Wine proxy for the vendor Windows application.

## Build and run

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"

phantom status
phantom light --mode wave --color ff0000 --level 4
sweep --seconds 60
badapple --start 1 --end 900
```

Run from the repo root so `mapping_custom.json` and `keymap_xy.json` resolve.
The keyboard must be accessible as `/dev/hidraw*` (interface 1, vendor usage
page `0xFF87`).

## Documentation

Everything learned about the hardware, firmware and protocol is in
[docs/](docs/README.md). Start with:

- [docs/device.md](docs/device.md) -- identification and limits
- [docs/protocol.md](docs/protocol.md) -- frame format, checksum, commands
- [docs/lighting.md](docs/lighting.md) -- effects, custom matrix, animation
- [docs/tft.md](docs/tft.md) -- the 128x128 screen

## Other pieces

- `phantom.py` -- Python reference implementation of the same protocol
- `probe.py`, `probe_preview.py`, `map_keys.py`, `align_keys.py`, `restore.py`
- `bridge/` + `app.sh` -- run the vendor Windows app under Wine

No vendor code or firmware is included. The Windows application is required
separately for the Wine path. This project is not affiliated with or endorsed
by Tecware. See [docs/legal.md](docs/legal.md) for scope and licensing notes.
