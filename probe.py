#!/usr/bin/env python3
"""Map custom-matrix LED indices to physical keys.

The custom matrix has 392 LEDs x 3 bytes (RGB) = 1176 bytes, written with
command 0x1A in 56-byte chunks. Unlike the preview buffer there is no alpha
and no per-frame streaming: the matrix is static, so one write per LED is
enough to hold it while you answer.

Close the vendor app first (./app.sh stop) or it will overwrite the matrix.

  python3 probe.py                # continue from mapping_custom.json
  python3 probe.py 60             # start at index 60
  python3 probe.py --reset        # start over

Enter = skip, !! = quit and save, "A+B" for one index lighting two keys.
"""
import json
import os
import sys

import phantom

COUNT = 392
ENTRY = 3
OUT = "mapping_custom.json"

args = sys.argv[1:]
if "--reset" in args:
    try:
        os.remove(OUT)
    except FileNotFoundError:
        pass
    args = [a for a in args if a != "--reset"]
START = int(args[0]) if args else 0

d = phantom.Phantom(phantom.find_device())


def setup():
    d._write_blob(0x1A, bytes(COUNT * ENTRY))       # all black
    a = bytearray(d.read_area())
    a[1] = 17      # mode = custom
    a[2] = 4       # brightness max
    a[5] = 0       # rgb is meaningless in custom mode
    d.write_area(bytes(a))


def show(idx):
    m = bytearray(COUNT * ENTRY)
    m[idx * 3:idx * 3 + 3] = b"\xff\xff\xff"
    d._write_blob(0x1A, bytes(m))


mapping = {}
if os.path.exists(OUT):
    mapping = json.load(open(OUT))
    print(f"resuming: {len(mapping)} entries in {OUT}")

setup()
print("Type the key name for the lit LED. Enter=skip, !! = quit+save.\n")
try:
    for idx in range(START, COUNT):
        if str(idx) in mapping:
            continue
        show(idx)
        ans = input(f"LED {idx:3d}: ").strip()
        if ans == "!!":
            break
        if ans:
            parts = [p.strip() for p in ans.replace(",", "+").split("+") if p.strip()]
            mapping[str(idx)] = parts[0] if len(parts) == 1 else parts
            with open(OUT, "w") as fh:
                json.dump(mapping, fh, indent=1)
finally:
    a = bytearray(d.read_area())
    a[1] = phantom.MODES["wave"]
    d.write_area(bytes(a))
    d.close()

print(f"\nsaved {len(mapping)} entries to {OUT}")
