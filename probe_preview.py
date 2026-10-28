#!/usr/bin/env python3
"""Interactively map preview LED indices to physical keys.

Lights one LED at a time and waits for you to type which key it is.
Progress is saved to mapping_preview.json after every entry, so you can quit and
resume later.

  python3 probe.py                 # start at 0
  python3 probe.py 20              # start at LED 20
  python3 probe.py 20 my.json      # custom output file

Enter = skip this LED, !! = quit and save.
"""
import json
import os
import sys
import threading
import time

import phantom

START = int(sys.argv[1]) if len(sys.argv) > 1 else 0
OUT = sys.argv[2] if len(sys.argv) > 2 else "mapping_preview.json"

d = phantom.Phantom(phantom.find_device())


def push(frame):
    """Send a frame twice, tolerating transient rejects."""
    for _ in range(2):
        try:
            d.sync_frame(frame)
        except OSError:
            time.sleep(0.05)


# Force a fully black board first: custom mode, brightness max, every
# stored LED black. Otherwise the firmware's own effect shows through the
# slots a preview frame doesn't cover.
mat = bytearray(phantom.LIGHT_COUNT * phantom.LIGHT_ENTRY)
for i in range(phantom.LIGHT_COUNT):
    mat[i * 3:i * 3 + 3] = b"\x00\x00\x00"
d.write_matrix(bytes(mat))
# The left edge is a separate side/logo light with its own effect; turn it
# off or it keeps showing rainbow over whatever the preview draws.
a = bytearray(d.read_area())
a[34] = 18          # sideMode = off
a[35] = 0           # sideRgb
a[36] = a[37] = a[38] = 0
a[40] = 0           # sideLevel
a[25] = 18          # logoMode = off
a[28] = a[29] = a[30] = 0
a[32] = 0           # logoLevel
d.write_area(bytes(a))
d.set_light(mode=phantom.MODES["custom"], level=4)

state = {"frame": None, "run": True}


def sender():
    while state["run"]:
        f = state["frame"]
        if f is not None:
            try:
                push(f)
            except Exception:
                pass
        time.sleep(0.1)


threading.Thread(target=sender, daemon=True).start()

mapping = {}
if os.path.exists(OUT):
    try:
        mapping = json.load(open(OUT))
        print(f"resuming: {len(mapping)} entries already in {OUT}")
    except Exception:
        mapping = {}

print("Type the key name for the lit LED. Enter=skip, !! = quit+save.\n")
try:
    for idx in range(START, phantom.PREVIEW_COUNT):
        if str(idx) in mapping:
            continue
        f = bytearray(phantom.PREVIEW_COUNT * phantom.PREVIEW_ENTRY)
        for i in range(phantom.PREVIEW_COUNT):
            f[i * 3:i * 3 + 3] = b"\x00\x00\x00"
        f[idx * 3:idx * 3 + 3] = b"\xff\xff\xff"
        state["frame"] = bytes(f)
        # the keeper thread holds it; just give the device a moment
        time.sleep(0.4)
        ans = input(f"LED {idx:2d}: ").strip()
        # "!!" quits so that typing the Q key still works
        if ans == "!!":
            break
        if ans:
            parts = [p.strip() for p in ans.replace(",", "+").split("+") if p.strip()]
            mapping[str(idx)] = parts[0] if len(parts) == 1 else parts
            with open(OUT, "w") as fh:
                json.dump(mapping, fh, indent=1)
finally:
    state["run"] = False
    time.sleep(0.2)
    try:
        d.stop_sync()
    except Exception:
        pass
    d.close()

print(f"\nsaved {len(mapping)} entries to {OUT}")
