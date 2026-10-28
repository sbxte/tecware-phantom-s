# References

## Primary sources

Vendor application files (shipped with `PhantomS Software.exe`; not committed):

| file | what it provided |
|---|---|
| `config/DeviceData/KF010/Config.json` | VID/PID, interface, usage page/usage, device links |
| `config/DeviceData/KF010/LedInfo.xml` | 19 effect IDs, brightness/speed maxima, defaults |
| `xml/RGB-87K-Keyboard-KF010.xml` | key names and layout coordinates |
| `xml/RGB-99K-Keyboard-GK108.xml`, `RGB-99K-Keyboard-ITK100.xml`, `RGB-108K-Keyboard.xml` | other layouts using the same protocol |

Captures and descriptors (working-directory files, not committed):

| file | contents |
|---|---|
| `iface1.decode.txt` | decoded interface-1 HID report descriptor (report 6/7/10) |
| `hid2.decode.txt`, `iface0.decode.txt` | interface-0 descriptor |
| `bridge/hidrawd.log` | timestamped hex frames from the Wine daemon |
| `hidapi_shim.log` | shim-side logging |

Mappings produced during the work:

| file | contents |
|---|---|
| `mapping_custom.json` | custom-matrix slot -> key name |
| `keymap_xy.json` | key name -> normalised photo coordinate |
| `keyboard_state.json` | snapshot of the 128-byte area and 1176-byte matrix |

## Protocol references

- `@ichipcn/hid-sdk` -- same framing, 16-bit checksum, 3-byte key entries:
  <https://www.npmjs.com/package/@ichipcn/hid-sdk>
- USB HID 1.11 specification:
  <https://www.usb.org/sites/default/files/hid1_11.pdf>
- Linux hidraw documentation:
  <https://docs.kernel.org/hid/hidraw.html>
- CRC-16/MODBUS parameters (poly `0x8005` reflected, init `0xFFFF`):
  <https://reveng.sourceforge.io/crc-catalogue/16.htm>

## Related projects

- OpenRGB -- no detector for `379a:0920` at the time of writing:
  <https://openrgb.org/> and <https://gitlab.com/CalcProgrammer1/OpenRGB>
- dokutan/rgb_keyboard -- Sonix/EVision family, different framing:
  <https://github.com/dokutan/rgb_keyboard>
- hidapi (C library and DLL ABI used by the shim):
  <https://github.com/libusb/hidapi>

## Tooling used

- `hidapi` Rust crate: <https://crates.io/crates/hidapi>
- `clap` argument parser: <https://docs.rs/clap>
- `image` crate (resize, GIF decode): <https://github.com/image-rs/image>
- `gif` crate: <https://github.com/image-rs/image-gif>
- `anyhow`, `serde_json`, `ctrlc`: <https://crates.io>
- Wine: <https://www.winehq.org/>
- mingw-w64 cross compiler: <https://www.mingw-w64.org/>
- ImageMagick (used by the Python reference for image/GIF conversion):
  <https://imagemagick.org/>
- `mpv` / `ffplay` (audio playback in the Bad Apple animation):
  <https://mpv.io/>, <https://ffmpeg.org/>

## Media

- Bad Apple frames and audio are not distributed with this repository. The
  animation binary reads a local PGM sequence (480x360, 30 fps) and any audio
  file supported by `mpv` or `ffplay`.

## Legal

See [legal.md](legal.md) for what is and is not distributed, reverse
engineering considerations, trademarks and third-party licenses.

## Notes on attribution

The protocol was derived from the device and from the sources above. No vendor
source code, firmware image or application binary is included in this
repository. The vendor application is required separately if you want to use
the Wine path.
