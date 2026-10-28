# Keymap, macros, profile, battery

These commands share report 6 but use different address spaces from the
lighting area.

## Keymap layers

| layer | default | read | write | size |
|---|---|---|---|---|
| Windows | `0x16` | `0x17` | `0x18` | 128 entries x 3 bytes = 384 B |
| Windows-FN | `0x32` | `0x33` | `0x34` | 384 B |

Entry layout is three bytes:

```
[type] [low] [high]
```

Example values read from a stock layer:

```
key   0: type=0x10 low=0x00 high=0x29
key   1: type=0x10 low=0x00 high=0x3a
key   2: type=0x10 low=0x00 high=0x3b
```

The exact meaning of `type` and the split between `low`/`high` was not fully
decoded; the important facts are the entry size, the count, and that the
current layer can be compared byte-for-byte with the default. The tool treats
the blob as opaque on save/load:

```bash
phantom keymap --layer win --save keys.json
phantom keymap --layer win --load keys.json
phantom keymap --layer win-fn --save fn.json
```

Mac layers (`0x24`, `0x25`-`0x2A`, `0x2B`-`0x2E`) are rejected with `0x0f` on
this firmware, so the app's Mac keymap tabs are inert here.

### Windows-key lock

Function-area byte 12 (`lockWin`) locks the Windows key: `1` locks, `0`
unlocks. It is part of the function area rather than the keymap blob.

## FN-lock list

`0x35` reads a 256-byte table. On the observed unit 222 of 256 bytes were
non-zero, but a write command for this table was not identified; the tool
therefore only reads and saves it:

```bash
phantom fnlock
phantom fnlock --save fnlock.bin
```

The first 32 bytes observed were:

```
2f602b000120700002206f000310082b0410042b0520b6000620cd000720b500
```

The structure is not decoded. It is almost certainly a list of which keys are
"FN-locked" (behave as their Fn function without holding Fn), but the field
layout is unknown.

## Macro region

| command | operation | size |
|---|---|---|
| `0x1B` | read | 2048 bytes |
| `0x1C` | write | 2048 bytes |

The observed unit had an all-zero macro region (0/2048 non-zero). The macro
encoding is unknown; the tool round-trips the blob so a capture made with the
vendor app can be backed up and restored:

```bash
phantom macro --save macros.bin
phantom macro --load macros.bin
```

There is no documented per-macro index command; it is a single flat region.

## Profile

| command | operation | size |
|---|---|---|
| `0x30` | read profile index | 1 byte |
| `0x31` | write profile index | 1 byte |

The observed index was 0. Write was implemented as a single-byte blob but not
verified against the board, so treat `phantom profile --set N` as experimental.

## Battery

`0x1F` returns 2 bytes:

```
[percent] [flag]
```

Observed while charging-ish/idle: `64 02` = 100%, flag 2. The meaning of the
flag is **inferred** (likely charging/connection state); only the percent byte
is treated as reliable.

## Additional observations

- The vendor `Config.json` sets `HidDeviceType`/`index` to 0, and the same
  protocol serves 87-, 99- and 108-key layout XMLs. The keymap blob is
  fixed-size regardless of layout; unused entries are default/zero.
- Reading a layer that has never been customised returns the same bytes as its
  default command, which is a cheap way to detect "stock" state.
- The macro and FN-lock regions survive normal lighting changes but are
  restored by the hardware reset described in [device.md](device.md).
