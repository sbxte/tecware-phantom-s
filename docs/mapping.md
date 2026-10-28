# LED mapping and animations

## Two independent index spaces

The firmware exposes two per-key buffers with unrelated numbering:

| buffer | command | slots | persistence |
|---|---|---|---|
| custom matrix | `0x1A` | 392 | stored |
| preview | `0x1D` | 84 | transient, streamed |

A physical key has a different index in each. The mapping files here describe
the **custom matrix**, because that is what the animations use. The preview map
was rebuilt with the corrected 3-byte format and is written to
`mapping_preview.json`; it is not used by the shipped animations.

## Custom matrix mapping

`probe.py` lights one matrix slot at a time and asks which physical key it is.
It is interactive and user-paced: the board holds the LED until you answer,
and progress is saved after every entry.

```bash
python3 probe.py            # resume from mapping_custom.json
python3 probe.py 60         # start at slot 60
python3 probe.py --reset    # start over
```

It first blacks the whole matrix and switches to custom mode. Input:

- `Enter` -- skip this slot
- `!!` -- save and quit
- a key name -- record it
- `A+B` -- record one slot that lights two keys

Output `mapping_custom.json` is an object keyed by decimal slot index:

```json
{
  "0": "Esc",
  "1": "F1",
  "16": "~",
  "33": "Q"
}
```

Current file: 83 entries, slots 0-95, of which 81 names also exist in
`keymap_xy.json`. The physical 81 keys therefore sit in a sparse subset of the
392 slots; the rest are unused, side/logo-adjacent, or physically absent.

### Filtered entries

Three pseudo-names appear in early probe data and are ignored by every
consumer:

```
"red somehow"   "faint red"   "[]"
```

The first two were misreadings of the firmware status LEDs bleeding through;
`[]` is an empty answer. They are not real keys.

## Photo XY mapping

`mapping_custom.json` only says which key an index is. To animate spatially we
also need where that key is in a photograph of the board.

`keymap_xy.json`:

```json
{
  "image": "IMG_20261028_060326588~2.jpg",
  "keys": {
    "Q": [0.3123, 0.2711],
    "W": [0.3510, 0.2711]
  }
}
```

Coordinates are normalised to the photo (0-1). The source photo is not
committed.

Pipeline:

1. `map_keys.py` serves a local browser page with the photo; click each key in
   order and it writes `keymap_xy.json`.
2. `align_keys.py` snaps each row to its median Y, treats Left/Down/Right as a
   shared row, and places Up above the centre column. This removes
   click jitter and forms the arrow T.

`Layout` in the core library joins the two files: for each matrix index it
takes the key name from `mapping_custom.json`, looks up `(x, y)` in
`keymap_xy.json`, discards the filtered entries, and normalises the whole set
to its own bounding box. That gives every lit slot an `(x, y)` in 0-1 without
depending on the photo's crop.

## Bad Apple

`anim/badapple` renders the video onto the keys.

Inputs:

- PGM frames: 480x360 binary P5, 8-bit, 6572 frames at 30 fps (about 219 s)
- audio: any file `mpv`/`ffplay` can play
- the same `mapping_custom.json` + `keymap_xy.json`

Defaults are built from `$HOME/Programming/contrib/badapple/`; override with
`--frames`/`--audio` or `BADAPPLE_FRAMES`/`BADAPPLE_AUDIO`.

Rendering:

1. Load the PGM header (`P5`, width, height, maxval 255) and pixels.
2. For each mapped LED, convert its normalised `(x, y)` to a pixel coordinate
   in the frame.
3. The pixel is lit if its value is `>= --threshold` (default 128); `--invert`
   flips black/white.
4. Lit slots are white `ff ff ff`; everything else is black.

Playback:

- Audio starts at `(start - 1) / 30` seconds via `mpv --no-video` or
  `ffplay -nodisp`.
- Frames are timed against `Instant`, not against render cost.
- If rendering falls more than 150 ms behind, frames are skipped to keep audio
  sync (`n = start + now * fps`).
- First frame is a full matrix write; the rest are delta writes (changed LEDs
  only), paced by `--packet-delay` (1 ms default).
- `--mode full` forces full-frame writes.
- `--every N` renders every Nth frame while keeping the 30 fps time base.
- `--dry-run` computes output without touching the device; `--frame N
  --dry-run` reports the lit-LED count for one frame.
- Before starting it snapshots the device; on exit or Ctrl-C it restores the
  exact previous area and matrix.

## Sweep

`anim/sweep` is the small spatial demo:

- A rainbow band about 3 keys wide moves left to right over `--period`
  seconds (default 14), then a black gap (`--pause`, default 2).
- Hue is computed across the band width; LEDs outside the band are black.
- Uses the same `Layout` positions and delta writes.
- Defaults to 300 s or `--once`; snapshots and restores state.

## Performance notes

- A full matrix is 21 packets. Each packet can cause a partial panel update,
  so full-frame animation tears.
- Delta writes reduce each frame to the changed LEDs, usually 1-3 packets for
  Bad Apple.
- Runs are capped at 54 bytes and aligned to whole LEDs so the firmware reads
  complete RGB triples.
- About 1 ms between packets is the practical minimum for smooth output at
  20-30 fps. Removing the delay does not make the panel faster; it only
  increases the chance of interleaved repaints.
- Reading the device is much slower than raw writing; reads are only used for
  setup, snapshots and diagnostics. Animations use raw `hid_write`.
