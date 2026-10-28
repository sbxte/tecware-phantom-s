#!/usr/bin/env python3
"""Decode hidrawd.log frames into something readable.

Usage:
  decode.py bridge/hidrawd.log
  decode.py bridge/hidrawd.log --follow
  decode.py bridge/hidrawd.log --cmd 15 --dir TX
  decode.py bridge/hidrawd.log --hex
"""
import argparse
import re
import sys
import time

USAGE = {
    0x04: "a", 0x05: "b", 0x06: "c", 0x07: "d", 0x08: "e", 0x09: "f",
    0x0a: "g", 0x0b: "h", 0x0c: "i", 0x0d: "j", 0x0e: "k", 0x0f: "l",
    0x10: "m", 0x11: "n", 0x12: "o", 0x13: "p", 0x14: "q", 0x15: "r",
    0x16: "s", 0x17: "t", 0x18: "u", 0x19: "v", 0x1a: "w", 0x1b: "x",
    0x1c: "y", 0x1d: "z",
    0x1e: "1", 0x1f: "2", 0x20: "3", 0x21: "4", 0x22: "5", 0x23: "6",
    0x24: "7", 0x25: "8", 0x26: "9", 0x27: "0",
    0x28: "Enter", 0x29: "Esc", 0x2a: "Backspace", 0x2b: "Tab", 0x2c: "Space",
    0x2d: "-", 0x2e: "=", 0x2f: "[", 0x30: "]", 0x31: "\\", 0x32: "#",
    0x33: ";", 0x34: "'", 0x35: "`", 0x36: ",", 0x37: ".", 0x38: "/",
    0x39: "Caps", 0x46: "PrtSc", 0x47: "ScrLk", 0x48: "Pause",
    0x49: "Insert", 0x4a: "Home", 0x4b: "PgUp", 0x4c: "Delete",
    0x4d: "End", 0x4e: "PgDn", 0x4f: "Right", 0x50: "Left",
    0x51: "Down", 0x52: "Up", 0x53: "NumLk", 0x54: "KP/", 0x55: "KP*",
    0x56: "KP-", 0x57: "KP+", 0x58: "KPEnter", 0x59: "KP1", 0x5a: "KP2",
    0x5b: "KP3", 0x5c: "KP4", 0x5d: "KP5", 0x5e: "KP6", 0x5f: "KP7",
    0x60: "KP8", 0x61: "KP9", 0x62: "KP0", 0x63: "KP.", 0x64: "",
    0x65: "Menu", 0x66: "Power", 0x67: "KP=", 0x68: "F13", 0x69: "F14",
    0x6a: "F15", 0x6b: "F16", 0x6c: "F17", 0x6d: "F18", 0x6e: "F19",
    0x6f: "F20", 0x70: "F21", 0x71: "F22", 0x72: "F23", 0x73: "F24",
}
for i, n in enumerate(range(0x3a, 0x46)):
    USAGE[n] = f"F{i + 1}"

LIGHT_FIELDS = [
    "mode", "light", "speed", "fx", "7color",
    "R", "G", "B", "colorIdx", "openFlag", "customMode", "ret1", "ret2",
]

CMD_NAMES = {
    0x12: "info",
    0x14: "light-read",
    0x15: "light-write",
    0x16: "keymap",
    0x17: "keymap-default",
    0x19: "region19",
    0x1b: "region1b",
}


def checksum(b):
    return (sum(b[1:5]) + sum(b[8:])) & 0xFF


def decode_keymap(data, base):
    out = []
    for i in range(0, len(data) - 2, 3):
        typ, lo, hi = data[i], data[i + 1], data[i + 2]
        if typ == 0 and lo == 0 and hi == 0:
            continue
        name = USAGE.get(hi, "")
        out.append(f"#{base // 3 + i // 3} t={typ:#04x} c={hi:#04x} {name}")
    return out


def decode_frame(t, direction, b, show_hex=False):
    when = time.strftime("%H:%M:%S", time.localtime(t))
    if len(b) < 8:
        return f"{when} {direction} short {b.hex()}"
    rid, cmd, addr, ln, cs, f6, f7 = b[0], b[1], b[2] | b[3] << 8, b[4], b[5], b[6], b[7]
    data = b[8:8 + ln]
    name = CMD_NAMES.get(cmd, "?")
    if direction == "TX":
        csum_ok = checksum(b) == cs
        csum_s = "csum=ok" if csum_ok else f"csum=BAD({cs:#04x}!={checksum(b):#04x})"
    else:
        csum_s = "echo"
    parts = [
        f"{when}", direction,
        f"id={rid:#04x}", f"cmd={cmd:#04x}", name,
        f"addr={addr:#06x}", f"len={ln}", csum_s,
    ]
    if f6:
        parts.append(f"f6={f6:#04x}")
    if f7 and f7 != 0x55:
        parts.append(f"f7={f7:#04x}")

    is_write = (cmd == 0x15 and direction == "TX")
    is_read_reply = (cmd in (0x14, 0x16, 0x17) and direction == "RX")
    is_light = is_write or (cmd == 0x14 and direction == "RX")

    if is_light and len(data) >= 13:
        # byte 8 is a section id, LightInfo starts at byte 9
        light = data[1:14]
        fields = " ".join(
            f"{n}={v}" if n in ("R", "G", "B") else f"{n}={v}"
            for n, v in zip(LIGHT_FIELDS, light)
        )
        parts.append(fields)
        if len(data) > 14:
            parts.append("sys=" + data[14:22].hex())
    elif cmd in (0x16, 0x17) and is_read_reply:
        keys = decode_keymap(data, addr)
        if keys:
            parts.append(" ".join(keys[:8]))
            if len(keys) > 8:
                parts.append(f"(+{len(keys) - 8} keys)")
    elif cmd == 0x15 and direction == "RX":
        parts.append("ack")
    elif any(data):
        parts.append("data=" + data[:24].hex())

    if show_hex:
        parts.append("raw=" + b.hex())
    return " ".join(parts)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log", help="path to hidrawd.log, or - for stdin")
    ap.add_argument("--follow", "-f", action="store_true", help="keep reading appended lines")
    ap.add_argument("--cmd", type=lambda s: int(s, 0), help="only this command, e.g. 0x15")
    ap.add_argument("--dir", choices=["TX", "RX"], help="only this direction")
    ap.add_argument("--hex", action="store_true", help="append raw frame hex")
    args = ap.parse_args()

    f = sys.stdin if args.log == "-" else open(args.log, "r")
    if args.log != "-" and args.follow:
        f.seek(0, 2)

    line_re = re.compile(r"(\d+\.\d+)\s+(TX|RX)\s+len=\s*(\d+)\s+([0-9a-f]+)")
    try:
        while True:
            line = f.readline()
            if not line:
                if args.follow:
                    time.sleep(0.2)
                    continue
                break
            m = line_re.search(line)
            if not m:
                continue
            t, direction, _ln, hexdata = m.groups()
            b = bytes.fromhex(hexdata)
            if args.cmd is not None and len(b) > 1 and b[1] != args.cmd:
                continue
            if args.dir and direction != args.dir:
                continue
            print(decode_frame(float(t), direction, b, args.hex), flush=True)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
