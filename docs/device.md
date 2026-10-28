# Device

## Identification

```
Manufacturer:  I-CHIP
Product:       Phantom S
VID:PID:       379a:0920
bcdDevice:     1.13
Interfaces:    2
```

The vendor data (`config/DeviceData/KF010/Config.json`) lists three device
links:

| VID | PID | interface | usage page | usage |
|---|---|---|---|---|
| `0x379a` | `0x0920` | 1 | `65415` = `0xFF87` | `32` = `0x20` |
| `0x379a` | `0x0920` | 1 | `65416` = `0xFF88` | `33` = `0x21` |
| `0x379a` | `0x0921` | 1 | `65415` = `0xFF87` | `32` = `0x20` |

This tool targets the first link: **interface 1, usage page `0xFF87`,
usage `0x20`**. The device is resolved by VID/PID/interface, never by
`/dev/hidrawN`, because node numbering changes across replugs.

Interface 0 carries the ordinary keyboard/mouse/consumer reports. Interface 1
is a composite that also carries the vendor reports; on Linux the two
interfaces are separate hidraw nodes and only interface 1 speaks the lighting
protocol.

## HID report descriptor

From the interface-1 descriptor dump (`iface1.decode.txt`):

```
Usage Page (Vendor 0xff87), Usage (0x20)
  Report ID 6, Report Size 8, Report Count 63
  Input  (Data,Arr,Abs)
  Output (Data,Arr,Abs)

Usage Page (Vendor 0xff88), Usage (0x21)
  Report ID 7, 2 bytes
  Input

Usage Page (Vendor 0xff00), Usage 1
  Report ID 10 (0x0A), 63 bytes
  Input and Output
```

So:

| report | page / usage | size | status |
|---|---|---|---|
| 6 | `0xFF87` / `0x20` | 63 in + 63 out | the lighting/control protocol documented here |
| 7 | `0xFF88` / `0x21` | 2 in | not used by this tool |
| 10 (`0x0A`) | `0xFF00` / 1 | 63 in + 63 out | **unexplored**; the only remaining channel that might expose status LEDs or telemetry |

The vendor application sends every lighting frame as report 6: one report-ID
byte plus 63 payload bytes, hence 64-byte writes and reads.

## Physical / logical specification

| property | value |
|---|---|
| layout | 75% ANSI, 81 keys (as mapped) |
| custom-matrix slots | 392 logical RGB entries |
| preview-buffer slots | 84 RGB entries |
| effect modes | 19 (IDs 0-18) |
| brightness | 0-4 |
| speed | 0-4 |
| direction | 0-7 |
| screen | 128 x 128 TFT, RGB565 |
| status LEDs | Caps (`A`), connection mode (`M`), battery |
| default effect | mode 1, brightness 4, speed 2 (vendor config) |

The vendor `LedInfo.xml` advertises `brightness_max="4"`, `speed_max="4"`,
`fx_max="1"`, `default_mode="1"`, `default_brightness="4"`,
`default_speed="2"`, and lists the 19 mode IDs.

The layout XML shipped for this model (`RGB-87K-Keyboard-KF010.xml`) contains
87 named key entries; the physical board exposes 81 keys in the mapping. Some
XML entries are for regions/keys this variant does not have.

## Timers and wireless fields

The function area carries four pairs of little-endian 16-bit timers even on
the wired model:

| offset | field |
|---|---|
| 15 | 2.4G LED-off timer |
| 17 | 2.4G sleep timer |
| 19 | Bluetooth LED-off timer |
| 21 | Bluetooth sleep timer |
| 23 | USB LED-off timer (default 600 s) |

The vendor UI displays the first four as zero and sends zeros for them; the
daemon preserves the cached values (see [wine.md](wine.md)).

## Known limits

- **392 custom slots, not 170.** Writes past byte 1176 are rejected with
  result `0x0f`.
- **No realtime colour read.** `0x21` ("read backlight matrix") returns zeros
  on this firmware.
- **Status LEDs are firmware-owned.** Their colours follow device state, not
  the matrix (see [lighting.md](lighting.md)).
- **Logo and side matrices are not host-addressable.** The mode fields exist
  but the per-LED read/write commands for those zones are rejected.
- **Mac keymap layers are rejected** (`0x0f`); only Windows and Windows-FN
  layers respond.
- **The preview buffer is only 84 slots** and is transient; it is not a second
  custom matrix.
- **Screen writes are one-way.** No screen read-back command was found.

## Recovery

If the board is left in an odd state (timers zeroed, effect stuck), the vendor
documented hardware reset is:

```
hold Fn + Esc + F1 + F3 + F5 for about 3 seconds
```

This restores default timers and lighting. It is the only recovery method
known to work when the host-side state cache is wrong.
