# Protocol

All lighting, keymap, macro and screen traffic uses HID report ID 6 on
interface 1: a 64-byte report with a 7-byte header, up to 56 bytes of payload,
and a 16-bit checksum.

## Frame layout

```
byte 0     0x06                 report ID
byte 1     command              e.g. 0x14 read area, 0x15 write area
byte 2     address low
byte 3     address high         16-bit little-endian address
byte 4     length               payload bytes in this frame (0..56)
byte 5     checksum low
byte 6     checksum high
byte 7     result               0 in requests; 0x55 ok; 0x0f rejected
byte 8..   payload              up to 56 bytes, zero padded
```

The address is the offset inside the command's address space, not a packet
number. A command is chunked by sending multiple frames with increasing
addresses.

## Checksum

```
csum = (sum(frame[1..5]) + sum(frame[7..64])) & 0xFFFF
```

In other words: the command, both address bytes, the length, the result byte
(normally 0 in a request), and the entire 56-byte payload including padding.
The two checksum bytes themselves are excluded.

This is the single most important detail in the protocol. An 8-bit sum (or an
8-bit checksum over just the payload) appears to work for reads because read
requests carry a zero payload, so the high byte is usually zero. Writes with
real data are silently ignored unless the full 16-bit sum is present.

Python:

```python
csum = (sum(b[1:5]) + sum(b[7:])) & 0xffff
b[5] = csum & 0xff
b[6] = csum >> 8
```

## Replies and result codes

Every command that expects an answer echoes the same command byte and places a
result at byte 7 and the reply length at byte 4:

| result | meaning |
|---|---|
| `0x00` | request direction; not a reply |
| `0x55` | accepted |
| `0x0f` | rejected / unsupported |

The read path matches on `buf[0] == 0x06 && buf[1] == cmd`, then returns
`buf[8 .. 8+len]`. A rejected command aborts immediately. A missing reply is
retried up to three times.

Timeouts used by this tool:

| operation | timeout |
|---|---|
| function area / device info | 600 ms |
| custom matrix and other blobs | 800 ms |
| screen open/data | 3000 ms |
| screen commit | 6000 ms |
| per-read poll in the daemon | 200 ms |

Writes that do not need a reply (the animation delta path) call
`hid_write`/`os.write` directly and do not wait. This matters for frame rate.

## Function area

A 128-byte block read with `0x14` and written with `0x15`, always in three
chunks at addresses 0, 56 and 112 (56 + 56 + 16 bytes).

| offset | size | field | notes |
|---|---|---|---|
| 0 | 1 | profile index | 0 on the observed unit |
| 1 | 1 | mode | 0-18, see [lighting.md](lighting.md) |
| 2 | 1 | brightness | 0-4 |
| 3 | 1 | speed | 0-4 |
| 4 | 1 | direction / trend | 0-7, see [lighting.md](lighting.md) |
| 5 | 1 | rgb / use-7-colour | 0 single colour, 1 built-in cycle |
| 6-8 | 3 | colour R,G,B | used when byte 5 is 0 |
| 9 | 1 | colour index | table index, not used by this tool |
| 10 | 1 | trigger colour | click/reaction colour |
| 12 | 1 | lockWin | 1 locks the Windows key |
| 15,17,19,21,23 | 2 each | timers | little-endian, see [device.md](device.md) |
| 25 | 1 | logoMode | observed; logo commands otherwise rejected |
| 26 | 1 | osMode | SDK field map |
| 34 | 1 | sideMode | observed; side commands otherwise rejected |
| 28-30, 32 | | logo RGB + level | offsets used by `probe_preview.py`, **infers** |
| 35-38, 40 | | side rgb/flag + level | offsets used by `probe_preview.py`, **infers** |

The logo/side offsets are the ones the vendor probe scripts manipulated when
turning those zones off before preview mapping. They are not independently
confirmed as a complete zone API, because the dedicated logo/side matrix
commands are rejected.

### Partial-area quirk

The vendor application writes only the **first 56 bytes** of the area (one
report). The firmware commits the area only when all 128 bytes are written, so
that single write is acknowledged but not applied. The Wine daemon works
around this by reconstructing the other chunks from a cache. Host tools in
this repo always write all three chunks.

The same application also sends zeroed wireless/USB timers on many writes. The
daemon substitutes the cached values when it sees a zero pair at offsets
15/17/19/21/23.

## Command map

Probed on firmware 1.13:

| command | meaning | result |
|---|---|---|
| `0x12` | device info, 57 bytes | works |
| `0x14` / `0x15` | read / write function area (128 B) | works |
| `0x16` / `0x17` / `0x18` | Windows keymap: default / read / write (128 x 3 B) | works |
| `0x19` / `0x1A` | custom LED matrix read / write (392 x 3 B) | works |
| `0x1B` / `0x1C` | macro region read / write (2048 B) | works, empty |
| `0x1D` / `0x1E` | realtime preview frame / stop | works |
| `0x1F` | battery: percent, flag | works |
| `0x21` | read backlight matrix | returns zeros |
| `0x30` / `0x31` | profile index read / write | read works, write untested |
| `0x32` / `0x33` / `0x34` | Windows-FN keymap: default / read / write | works |
| `0x35` | FN-lock list, 256 B | read works |
| `0x40` / `0x41` / `0x42` | screen open / data / commit | works |
| `0x24`, `0x25`-`0x2A`, `0x2B`-`0x2E` | Mac keymap layers | rejected `0x0f` |
| `0x36`-`0x3C` | logo/side matrices, realtime colour read, magnetic | rejected `0x0f` |
| `0x55` | link probe | rejected `0x0f` |

Report `0x0A` on usage page `0xFF00` is present in the descriptor but was
never exercised; it is the only unexplored channel.

## Device info

`0x12` returns 57 bytes across two reads: 56 bytes at address 0 and 1 byte at
address 56. Observed fields:

| offset | value | meaning |
|---|---|---|
| 4 | 128 | keymap entry count |
| 48 | 0 | light entry size reported by firmware |
| 49-50 | 0 | light count reported by firmware (LE) |

The last two are zero on this board. The vendor SDK trusts them, which is why
that SDK cannot size the custom matrix; the real size has to be probed.

## Design notes

- There is no "transaction ID" field; replies are matched by command byte and
  must be read promptly or they queue up.
- Addresses wrap into a 16-bit space; the largest block used here is the
  1176-byte matrix.
- Every matrix/area write is a firmware operation that can trigger a partial
  panel update; this is the source of tearing, see [lighting.md](lighting.md).
