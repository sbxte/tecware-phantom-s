# Lighting

## Effect modes

Mode is function-area byte 1. The vendor `LedInfo.xml` enumerates the same
IDs; the internal names are included because they show up in the application's
resources.

| ID | name used here | vendor internal name |
|---|---|---|
| 0 | wave | `light_mode_wave` |
| 1 | cloud | `light_mode_caiyun` |
| 2 | neon | `light_mode_nihong` |
| 3 | vortex | `light_mode_huixuan` (rotate) |
| 4 | flower | `light_mode_baihua` |
| 5 | fly | `light_mode_feixiang` |
| 6 | flow | `light_mode_chuanyue` (traverse) |
| 7 | rain | `light_mode_rain` |
| 8 | mixcolor | `light_mode_guangpu` (spectrum) |
| 9 | breathe | `light_mode_breathe` |
| 10 | static | `light_mode_normal` |
| 11 | ripple | `light_mode_lianyi` |
| 12 | click | `light_mode_qiaoji` (AOE) |
| 13 | reaction | `light_mode_dandian` (single point) |
| 14 | speed | `light_mode_kuosan` (diffusion) |
| 15 | scan | `light_mode_scan` |
| 16 | change | `light_mode_jianbian` (gradient) |
| 17 | custom | `light_mode_custom` |
| 18 | off | `light_mode_off` |

Brightness (byte 2) and speed (byte 3) are 0-4. The tool clamps them.

## Direction

Function-area byte 4. The values below are **inferred** from the order of the
vendor string table; they were not verified motion-by-motion.

| value | direction |
|---|---|
| 0 | left to right |
| 1 | right to left |
| 2 | top to bottom |
| 3 | bottom to top |
| 4 | counter-clockwise |
| 5 | clockwise |
| 6 | inside to outside |
| 7 | outside to inside |

If a name produces unexpected motion, it is a one-line permutation; the field
is definitely byte 4 and the domain is definitely 0-7.

## Colour fields

- Byte 5 is `rgb` / "use 7-colour": `1` runs the firmware's built-in colour
  cycle for preset effects, `0` uses the single colour in bytes 6-8.
- Bytes 6-8 are R, G, B.
- Byte 9 is a colour-table index and byte 10 the trigger colour. Neither is
  used by this tool.
- The field has no meaning in custom mode; the vendor application pins it to 0
  there.

## Custom LED matrix

This is the persistent per-key layer used by "custom" mode.

| property | value |
|---|---|
| command read | `0x19` |
| command write | `0x1A` |
| entries | 392 |
| bytes per entry | 3 (R, G, B) |
| total size | 1176 bytes |
| chunk size | 56 bytes (21 frames for a full write) |
| mode | set byte 1 of the function area to 17 |

The matrix is static: once written it stays until overwritten, powered off, or
the profile changes. Unlike the preview buffer it does not need refreshing.

**Writes beyond 1176 bytes are rejected with `0x0f`.** Probing that boundary is
how the 392-entry size was found.

In captures, the vendor application normally writes only the first 128 entries
(384 bytes) for its UI grid; the remaining entries are still host-addressable. The 81
physical keys used by this repo occupy a sparse set of indices in 0..95, not
0..80.

### Historical format bug

The matrix was initially assumed to be 170 entries of RGBA (4 bytes). That
produced a repeating red/green/blue pattern and "static columnar bands" on the
board. The firmware was reading 3-byte RGB all along, so every fourth byte
shifted the colour of every following LED. Switching to 392 x RGB fixed both
the size limit and the bands. The same mistake was later found in the preview
buffer.

## Realtime preview buffer

| property | value |
|---|---|
| command frame | `0x1D` |
| command stop | `0x1E` |
| slots | 84 |
| bytes per slot | 3 (RGB) |
| total size | 252 bytes |

The preview buffer is a **transient overlay**, not stored state. The vendor
application uses it to show live slider changes before committing them. It
covers only 84 slots, so it is not a replacement for the 392-slot matrix, and
frames normally need to be re-sent while active. `probe_preview.py` streams a
frame from a background thread to hold a single lit slot while mapping.

The indices are a completely separate address space from matrix indices. A
mapping made against one is meaningless for the other.

## Animation

The panel is updated as packets arrive, not as an atomic frame. A full matrix
write is 21 packets; each packet can repaint part of the panel while the rest
still shows the previous frame. That is the tearing seen with naive full-frame
animation.

The working strategy is **delta writes**:

1. Keep the previous matrix in memory.
2. Find LEDs whose RGB changed.
3. Merge contiguous changed LEDs into runs, capped at 54 bytes (18 LEDs) so
   each run fits one packet with 3-byte alignment.
4. Send one raw `0x1A` packet per run, with about 1 ms between packets.
5. Pace frames at roughly 20-30 fps.

For Bad Apple most frames change only a handful of LEDs, so this is far
cheaper and smoother than rewriting 1176 bytes. The CLI/core library exposes
`write_delta` for this; `write_matrix_fast` writes full frames without waiting
for acknowledgements.

## Status indicators

The Caps (`A`), mode (`M`) and battery LEDs are **firmware-rendered** from
device state and are not part of the matrix. Observed meanings:

| colour | meaning |
|---|---|
| white | Caps Lock on |
| green | 2.4G connection |
| blue | Bluetooth |
| bright red | charging |
| dim red | idle |

The faint red under the bottom-row modifiers is light bleed from those status
LEDs, not a matrix entry. No host command found so far overrides them. Report
`0x0A` on usage page `0xFF00` is the only unexplored channel where such
control might live.

## Saving and restoring state

Animations snapshot the device before touching it:

```rust
let before = dev.snapshot()?;   // 128-byte area + 1176-byte matrix
dev.enter_custom(level)?;
...
dev.restore(&before)?;          // exact previous mode, colour, timers, matrix
```

This is why `sweep` and `badapple` return the board to whatever was there
before instead of leaving a green wave. They install a Ctrl-C handler so the
restore also happens on interrupt.
