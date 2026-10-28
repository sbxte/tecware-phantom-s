#!/usr/bin/env python3
"""Snap keymap_xy.json rows to a common Y (median of each row).

Rows are snapped to their median Y. The four arrows are treated as their own
upside-down-T cluster: Left/Down/Right share a Y, Up sits above the centre
column (same X as Down). X of letter keys is never changed.
A backup is written before modifying.
"""
import json
import shutil
import statistics

PATH = "keymap_xy.json"
BACKUP = "keymap_xy_before_align.json"

ROWS = {
    "function": ["Esc"] + [f"F{i}" for i in range(1, 13)] + ["Del"],
    "number":   ["~", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0",
                 "-", "=", "Backspace"],
    "qwerty":   ["Tab", "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P",
                 "[", "]", "\\", "Home"],
    "asdf":     ["Caps", "A", "S", "D", "F", "G", "H", "J", "K", "L", ";",
                 "'", "Enter", "PgUp"],
    "zxcv":     ["Shift", "Z", "X", "C", "V", "B", "N", "M", ",", ".", "/",
                 "RShift", "PgDn"],
    "mods":     ["LCtrl", "LWin", "LAlt", "Space", "RAlt", "Fn", "RCtrl"],
}
ARROWS = ["Up", "Down", "Left", "Right"]

data = json.load(open(PATH))
keys = data["keys"]

missing = [k for row in ROWS.values() for k in row if k not in keys]
if missing:
    print("not in keymap, skipping:", missing)

covered = {k for row in ROWS.values() for k in row}
extra = sorted(set(keys) - covered - set(ARROWS))
if extra:
    print("keys not covered by any row:", extra)

shutil.copy(PATH, BACKUP)
for name, members in ROWS.items():
    present = [k for k in members if k in keys]
    if not present:
        continue
    ys = [keys[k][1] for k in present]
    y = statistics.median(ys)
    spread = max(ys) - min(ys)
    for k in present:
        keys[k][1] = round(y, 6)
    print(f"{name:9s} y={y:.4f}  n={len(present):2d}  old spread={spread:.4f}")
    if name == "qwerty":
        print(f"          Home snapped to {y:.4f} (x stays {keys['Home'][0]:.4f})")
    if name == "asdf":
        print(f"          PgUp snapped to {y:.4f} (x stays {keys['PgUp'][0]:.4f})")
    if name == "zxcv":
        print(f"          PgDn snapped to {y:.4f} (x stays {keys['PgDn'][0]:.4f})")

if all(k in keys for k in ARROWS):
    bottom = ["Left", "Down", "Right"]
    ay = statistics.median(keys[k][1] for k in bottom)
    for k in bottom:
        keys[k][1] = round(ay, 6)
    cx = (keys["Left"][0] + keys["Right"][0]) / 2
    keys["Down"][0] = round(cx, 6)
    keys["Up"][0] = round(cx, 6)
    print(f"arrows    Left/Down/Right y={ay:.4f}, Up/Down x={cx:.4f} "
          f"(Up y={keys['Up'][1]:.4f})")

json.dump(data, open(PATH, "w"), indent=1, sort_keys=True)
print(f"\nwrote {PATH} (backup: {BACKUP})")
print("arrows:", {k: (round(keys[k][0], 4), round(keys[k][1], 4))
                  for k in ARROWS if k in keys})
