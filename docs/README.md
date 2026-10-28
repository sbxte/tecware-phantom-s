# Tecware Phantom S documentation

This is the full reverse-engineering record for the Tecware Phantom S
(`379a:0920`, I-CHIP `KF010`) as observed on firmware `bcdDevice 1.13`.

Everything here was obtained by observing USB traffic, probing commands,
reading the vendor application's data files, and writing code against the
device. No firmware was decompiled for this document; where a value is
inferred rather than confirmed it is marked **inferred**.

## Contents

| document | what it covers |
|---|---|
| [device.md](device.md) | USB IDs, interfaces, HID report descriptor, physical specs, hard limits |
| [protocol.md](protocol.md) | report 6 framing, 16-bit checksum, result codes, command map, function area |
| [lighting.md](lighting.md) | modes, direction, colours, custom LED matrix, preview buffer, animation strategy |
| [tft.md](tft.md) | the 128x128 screen: transport, packet CRC, image/GIF upload, clock |
| [keymap.md](keymap.md) | keymap layers, FN-lock list, macro region, profile, battery |
| [wine.md](wine.md) | running the vendor application under Wine via a `hidapi.dll` shim |
| [mapping.md](mapping.md) | LED-index mapping, photo XY mapping, Bad Apple and sweep pipelines |
| [history.md](history.md) | how the protocol was cracked, wrong turns, comparison with other projects |
| [references.md](references.md) | vendor files, SDKs, projects and specifications used |
| [legal.md](legal.md) | scope, EULA/RE notes, trademarks, what is not distributed |

## Quick facts

| | |
|---|---|
| USB VID:PID | `379a:0920` (config also lists `379a:0921`) |
| manufacturer / product | `I-CHIP` / `Phantom S` |
| firmware | `bcdDevice 1.13` |
| control interface | interface 1, usage page `0xFF87`, usage `0x20` |
| control report | report ID `6`, 63-byte payload (64-byte packets) |
| checksum | 16-bit sum, see [protocol.md](protocol.md) |
| custom LED slots | 392 x RGB = 1176 bytes |
| preview slots | 84 x RGB = 252 bytes |
| screen | 128 x 128, RGB565 big-endian |
| effects | 19 modes, brightness/speed 0-4, 8 directions |
| keys | 81-key 75% ANSI layout |

## Reading order

If you only want to drive the lighting, read [protocol.md](protocol.md) and
[lighting.md](lighting.md). If you want to animate, add
[mapping.md](mapping.md). The screen and keymap docs are independent of each
other.

The two most important findings, both of which cost significant time:

1. The checksum is a **16-bit sum**, not 8-bit. Reads succeeded with the low
   byte only because their payload is zero; writes were silently ignored.
2. The custom matrix is **392 entries of 3 bytes (RGB)**, not 170 or 128
   entries of 4 bytes (RGBA). The "static columnar bands" artifact was the
   firmware interpreting RGBA data as RGB, nothing more.
