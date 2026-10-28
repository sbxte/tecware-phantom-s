# TFT screen

The board has a 128x128 colour screen driven through the same report 6
transport, but with its own nested packet format.

## Transport

Screen transactions are chunked over three commands:

| command | role |
|---|---|
| `0x40` | open a transaction (first chunk) |
| `0x41` | continue a transaction |
| `0x42` | commit / finish |

Each chunk is at most one report payload: 56 bytes, minus any header that must
be repeated (GIF data repeats a 7-byte header, so those chunks carry 49 bytes).
The frame's address field carries the byte offset within the transaction.

A finish is sent as `0x42` with 56 zero bytes and a 6-second timeout.

## Nested packet

```
A5 5A  type  len_hi  len_lo  crc_hi  crc_lo  payload...
```

- `type` is the screen command (table below).
- `len` is the declared payload length, big-endian. For an image-data header
  this declares the total pixel length even though the packet itself is empty.
- CRC is CRC-16/MODBUS: initial value `0xFFFF`, polynomial `0xA001`
  (reflected `0x8005`), computed over exactly the three bytes
  `type, len_hi, len_lo`. It does **not** cover the payload.
- Pixel data is RGB565, **big-endian** (high byte first).
- The panel is 128 x 128; 128*128*2 = 32768 bytes of pixel data.

Reference implementation:

```python
def crc16(data):
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc
```

## Packet types

| type | name | payload |
|---|---|---|
| 9 | set time | hour, minute, second |
| 10 | set date | year-2000, weekday, month, day |
| 11 | home | none |
| 12 | image data | header declares total pixel bytes |
| 13 | switch to image | none |
| 14 | delete image | image slot (present in the vendor protocol; not used by this tool) |
| 15 | switch to GIF | none |
| 16 | GUI event | sub-command + arguments |
| 17 | GIF data | header declares one frame's bytes |
| 18 | GIF frames | slot, frame count |
| 19 | GIF FPS | slot, fps |

GUI-event sub-commands observed:

| value | meaning |
|---|---|
| 1 | image upload marker |
| 2 | GIF frame marker: `2, slot, frame_index` |

## Image upload sequence

```
open: GUI event [1]
sleep 350 ms
image-data header with declared pixel length
sleep 50 ms
pixel bytes, chunked to 56 bytes per report
sleep 500 ms
finish (0x42)
sleep 300 ms
optional: switch-to-image (type 13)
```

The pixel payload is RGB565 big-endian. In the Rust tool it comes from the
`image` crate (`resize_exact` to 128x128, then RGB565 conversion); the Python
reference shells out to ImageMagick.

## GIF upload sequence

GIF slots observed: `boot = 0`, `user = 1`, `home = 2`. The Rust CLI accepts
those names.

```
gif-frames [slot, 0]
gif-fps    [slot, 0]
sleep 30 ms
for each frame:
    GUI event [2, slot, frame_index]
    sleep 3000 ms every 16th frame, else 30 ms
    gif-data header declaring frame byte length
    frame bytes in chunks of 1024, each chunk repeating the 7-byte header
    sleep 30 ms between chunks when the frame is larger than 1024
gif-frames [slot, total_frames]
sleep 30 ms
gif-fps    [slot, fps]
sleep 30 ms
finish
optional: switch-to-gif (type 15)
```

The long 3-second pause every 16 frames is present in the vendor application
and appears to let the firmware flush its frame store. Removing it was not
tested.

## Clock

Set-time and set-date are ordinary screen transactions (open, one packet,
finish):

```
set time: [hour, minute, second]
set date: [year % 100, iso_weekday, month, day]
```

The tool derives these from the host clock; weekday is 1 = Monday through
7 = Sunday.

## Limits

- 128x128 only; larger images are resized.
- No screen read-back was found.
- Only one image and a small number of GIF slots are addressable; the exact
  flash budget is unknown.
- A failed screen transaction has no explicit error path beyond the normal
  `0x0f` reject; the tool retries at the report level.
