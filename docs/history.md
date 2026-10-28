# Reverse-engineering history

## Method

The board was treated as a black box:

1. Enumerate USB devices and decode the HID report descriptors from sysfs.
2. Capture hidraw traffic while exercising the vendor application under Wine.
3. Replay captured frames, then vary one byte at a time and observe the board.
4. Cross-check against the vendor's own data files (mode tables, layout XMLs)
   and against a public SDK that speaks the same protocol.
5. Build a small tool, then use it to probe the boundaries (matrix size,
   rejected commands).

No firmware was decompiled. Where the vendor application was examined, it was
through its shipped configuration and XML resources, not its code.

## Chronology

### Identification

The device is `379a:0920`, manufacturer `I-CHIP`, product `Phantom S`,
interface 1 using vendor usage page `0xFF87`. The vendor `Config.json` lists
the same usage page in decimal (`65415`) and a second link on `0xFF88`.

### Capturing

Wine's HID passthrough is broken (see [wine.md](wine.md)), so the initial
capture required owning the hidraw node directly and logging every report.
`iface1.decode.txt` contains the decoded report descriptor; `bridge/decode.py`
later decoded captured frames.

### The checksum

The first working reads used an 8-bit sum. Writes were acknowledged but never
applied. The reason is that read requests carry zero payload, so the high byte
of the true 16-bit sum is often zero; writes with real data were being sent
with a truncated checksum and silently dropped. Switching to the 16-bit sum
over `frame[1..5] + frame[7..]` made every previously "ignored" write work.

This was the single largest time sink of the project.

### Matrix size and format

The first assumption was 170 LEDs of RGBA (the number the vendor grid
suggested), later 128 (the vendor UI grid and the initial probe count). Both
were interpreted with 4-byte RGBA records and produced a
repeating red/green/blue pattern and static columnar bands. Probing writes at
increasing addresses until the firmware rejected them found the real boundary:
1176 bytes = 392 entries x 3 bytes RGB. The controller was reinterpreting
4-byte records as 3-byte RGB, shifting every colour.

The same mistake existed independently in the preview-buffer code, which used
84 x RGBA. It was corrected to 84 x RGB after the matrix was understood.

### Effects, TFT, keymap

Effect IDs came from the vendor `LedInfo.xml` and were confirmed by setting
each mode. The TFT protocol was reconstructed from screen traffic: outer
open/data/commit commands, an inner `A5 5A` packet with a CRC over three
bytes, and RGB565 pixels. The keymap, macro, FN-lock, profile and battery
commands were found by probing the command space and checking result bytes.

### Wine proxy

Because Wine registers hidraw nodes but not SetupAPI HID interfaces, the vendor
app saw no devices. A 32-bit `hidapi.dll` shim forwarding to a hidraw-owning
daemon made the app work. Two quirks had to be handled: the app writes only
the first 56 bytes of the 128-byte function area, and it sends zeroed timers.

### Mapping and animation

LED-to-key mapping was done by lighting one slot at a time and prompting for
the key. Spatial mapping was done from a photo in a browser clicker, then row
snapped. Bad Apple used the resulting XY map to sample PGM frames. Naive full
frame writes tore badly; delta writes (changed LEDs only, ~1 ms apart) made it
smooth.

## Wrong turns worth recording

| assumption | reality |
|---|---|
| checksum is 8-bit | 16-bit sum; reads hid the difference |
| matrix is 170 x RGBA | 392 x RGB |
| matrix is 128 x RGBA | same; 128 was the vendor UI grid, not the firmware limit |
| preview is 84 x RGBA | 84 x RGB |
| "columnar bands" are a firmware effect | RGBA data interpreted as RGB |
| SDK can size the light buffer | it reports lightCount = 0 on this board |
| app's partial area write succeeds | firmware acks but does not commit |
| status LEDs can be driven from the matrix | firmware-rendered, host-inaccessible so far |

## Differential analysis

- **OpenRGB** has no detector for `379a:0920` at the time of writing. Its
  EVision/Sonix controller targets a different vendor family
  (`0x320F`/`0x3299`, usage page `0xFF1C`), so it was not usable as a
  reference beyond general structure.
- **dokutan/rgb_keyboard** targets Sonix/EVision `0x0c45:652f`: 64-byte
  reports on EP `0x03`, a `04 01 00 01` start packet, `04 02 00 02` end
  packet, and 3-byte per-key entries. The 3-byte key entry is the same idea,
  but transport and framing are different.
- **`@ichipcn/hid-sdk`** is the same protocol family and was the most useful
  reference. Its frame layout, 3-byte key entries, and 16-bit checksum match.
  Its `DeviceInfo` reports `lightCount = lightEntrySize = 0` on this board, so
  it cannot size the custom matrix either; that had to be probed.

The SDK documents capabilities this firmware rejects: logo/side matrices,
realtime colour read, default-custom-light read, Mac keymap layers, and
magnetic switch travel. Those mismatches explain several dead ends.

## Unresolved

- **Report `0x0A`** (`0xFF00`/1, 63 bytes in/out) is in the descriptor but was
  never exercised. It is the only unexplored channel and a plausible home for
  status-LED or telemetry control.
- **Status indicators** remain firmware-owned. Their palette is known
  (white/green/blue/red) but no host override was found.
- **Logo and side matrices** are not addressable with the commands tried.
- **Mac keymap layers** are rejected.
- **Preview mapping** is not needed by the shipped animations; the map was
  regenerated with the correct stride but is not part of the main pipeline.
- **Keymap entry semantics** (`type`/`low`/`high`) and the **FN-lock** and
  **macro** formats are not decoded; the tools round-trip the blobs.
- **Profile write** (`0x31`) was implemented but not verified.
