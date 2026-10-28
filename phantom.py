#!/usr/bin/env python3
"""Tecware Phantom S (VID 379a PID 0920) lighting control for Linux.

Protocol reverse-engineered from the device and cross-checked against the
vendor SDK (@ichipcn/hid-sdk), which documents the same framing.

Report ID 6, 64-byte packets (1 ID + 63 payload):
    [0] 0x06            report id
    [1] command         0x14 read function area, 0x15 write function area
    [2] address lo
    [3] address hi
    [4] length          payload bytes in this packet
    [5] checksum lo     16-bit sum of payload bytes, excluding bytes 5 and 6
    [6] checksum hi
    [7] result          0 in requests, 0x55 = OK, 0x0f = rejected
    [8:] data           up to 56 bytes

Function area is 128 bytes, written in chunks at addresses 0, 56, 112.
Offsets (from the SDK's lighting/core field maps):
    0 profileIndex   1 mode   2 level(brightness)   3 speed   4 trend
    5 rgb (use colour)  6 R  7 G  8 B  9 colourIndex  10 triggerColour
    15 offLedTime24g (u16)  17 sleepTime24g  19 offLedTimeBle
    21 sleepTimeBle  23 offLedTimeUsb (u16)  26 osMode

Modes: 0 Wave, 1 Cloud, 2 Neon, 3 Vortex, 4 Flower, 5 Fly, 6 Flow, 7 Rain,
       8 MixColor, 9 Breathe, 10 Static, 11 Ripple, 12 Click, 13 Reaction,
       14 Speed, 15 Scan, 16 Change, 17 Custom, 18 Off
"""
import argparse
import os
import threading
import select
import sys
import time

VID = 0x379A
PID = 0x0920
REPORT_ID = 0x06
PACKET = 64
CAPACITY = 56          # 63 payload - 7 header
AREA = 128
RESULT_OK = 0x55
LIGHT_COUNT = 392
LIGHT_ENTRY = 3        # R, G, B per LED
PREVIEW_COUNT = 84     # 0x1d realtime frame buffer (84 * RGB = 252 bytes)
PREVIEW_ENTRY = 3      # R, G, B per slot (not RGBA)

# Screen (TFT) protocol
SCREEN_OPEN, SCREEN_DATA, SCREEN_FINISH = 0x40, 0x41, 0x42
PKT_SET_TIME, PKT_SET_DATE = 9, 10
PKT_HOME, PKT_IMAGE_DATA, PKT_SWITCH_IMAGE, PKT_DELETE_IMAGE = 11, 12, 13, 14
PKT_SWITCH_GIF, PKT_GUI_EVENT = 15, 16
PKT_GIF_DATA, PKT_GIF_FRAMES, PKT_GIF_FPS = 17, 18, 19
GUI_IMAGE_UPLOAD, GUI_GIF_FRAME = 1, 2
SLOTS = {"boot": 0, "user": 1, "home": 2}
SCREEN_DEFAULT = (128, 128)

MODES = {
    "wave": 0, "cloud": 1, "neon": 2, "vortex": 3, "flower": 4, "fly": 5,
    "flow": 6, "rain": 7, "mixcolor": 8, "breathe": 9, "static": 10,
    "ripple": 11, "click": 12, "reaction": 13, "speed": 14, "scan": 15,
    "change": 16, "custom": 17, "off": 18,
}
MODES_BY_ID = {v: k for k, v in MODES.items()}

DIRECTIONS = {
    "left-to-right": 0, "right-to-left": 1, "top-to-bottom": 2,
    "bottom-to-top": 3, "ccw": 4, "cw": 5, "in-to-out": 6, "out-to-in": 7,
}


def find_device():
    for name in sorted(os.listdir("/sys/class/hidraw")):
        uevent = f"/sys/class/hidraw/{name}/device/uevent"
        try:
            with open(uevent) as f:
                text = f.read()
        except OSError:
            continue
        if f"0003:{VID:08X}:{PID:08X}" not in text.upper():
            continue
        # interface 1 carries the vendor collections
        link = os.path.realpath(f"/sys/class/hidraw/{name}/device")
        if ":1.1" not in link:
            continue
        return f"/dev/{name}"
    return None


class Phantom:
    def __init__(self, path):
        self.fd = os.open(path, os.O_RDWR)
        self.path = path
        self._lock = threading.Lock()

    def close(self):
        os.close(self.fd)

    def _packet(self, cmd, data=b"", addr=0):
        b = bytearray(PACKET)
        b[0] = REPORT_ID
        b[1] = cmd
        b[2] = addr & 0xFF
        b[3] = (addr >> 8) & 0xFF
        b[4] = len(data)
        b[8:8 + len(data)] = data
        csum = (sum(b[1:5]) + sum(b[7:])) & 0xFFFF
        b[5] = csum & 0xFF
        b[6] = (csum >> 8) & 0xFF
        return bytes(b)

    def _exchange(self, cmd, data=b"", addr=0, timeout=0.6, tries=3):
        with self._lock:
            return self._exchange_locked(cmd, data, addr, timeout, tries)

    def _exchange_locked(self, cmd, data, addr, timeout, tries):
        for _ in range(tries):
            os.write(self.fd, self._packet(cmd, data, addr))
            deadline = time.time() + timeout
            while time.time() < deadline:
                left = max(0.0, deadline - time.time())
                r, _, _ = select.select([self.fd], [], [], left)
                if not r:
                    break
                d = os.read(self.fd, 256)
                if len(d) < 8 or d[0] != REPORT_ID:
                    continue
                if d[1] != cmd:
                    continue
                if d[7] != RESULT_OK:
                    raise IOError(f"device rejected cmd 0x{cmd:02x} addr {addr} "
                                  f"(result 0x{d[7]:02x})")
                return d[8:8 + d[4]]
        raise IOError(f"no reply for cmd 0x{cmd:02x} addr {addr}")

    def read_area(self):
        area = bytearray()
        addr = 0
        while addr < AREA:
            n = min(CAPACITY, AREA - addr)
            area += self._exchange(0x14, bytes(n), addr)
            addr += n
        return area

    def write_area(self, area):
        if len(area) != AREA:
            raise ValueError(f"area must be {AREA} bytes, got {len(area)}")
        for addr in range(0, AREA, CAPACITY):
            chunk = bytes(area[addr:addr + CAPACITY])
            self._exchange(0x15, chunk, addr)

    def set_light(self, mode=None, level=None, speed=None, color=None,
                  rgb=None, direction=None):
        area = self.read_area()
        if mode is not None:
            area[1] = mode & 0xFF
        if level is not None:
            area[2] = max(0, min(4, level))
        if speed is not None:
            area[3] = max(0, min(4, speed))
        if direction is not None:
            area[4] = direction & 0x07
        if rgb is not None:
            area[5] = 1 if rgb else 0
        if color is not None:
            area[5] = 0
            area[6], area[7], area[8] = color
        self.write_area(area)
        return area

    def status(self):
        return self.read_area()

    # ---- screen ----
    @staticmethod
    def _crc16(data):
        crc = 0xFFFF
        for b in data:
            crc ^= b & 0xFF
            for _ in range(8):
                crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
        return crc & 0xFFFF

    @classmethod
    def _screen_packet(cls, pkt_type, data=b"", declared=None):
        n = (len(data) if declared is None else declared) & 0xFFFF
        head = bytes([pkt_type, n >> 8, n & 0xFF])
        crc = cls._crc16(head)
        return bytes([0xA5, 0x5A]) + head + bytes([crc >> 8, crc & 0xFF]) + data

    def _screen_send(self, payload, repeat=None, opened=False, timeout=3.0):
        cap = CAPACITY - (len(repeat) if repeat else 0)
        addr = 0
        first = not opened
        while addr < len(payload):
            chunk = bytes(payload[addr:addr + cap])
            data = (repeat or b"") + chunk
            cmd = SCREEN_OPEN if first else SCREEN_DATA
            self._exchange(cmd, data, addr, timeout=timeout)
            first = False
            addr += len(chunk)

    def _screen_transaction(self, steps):
        opened = False
        for payload, repeat in steps:
            self._screen_send(payload, repeat, opened)
            opened = True
        self._exchange(SCREEN_FINISH, bytes(CAPACITY), 0, timeout=5.0)

    def screen_switch(self, page):
        self._screen_transaction([(self._screen_packet(page), None)])

    def screen_time(self):
        import datetime
        now = datetime.datetime.now()
        t = bytes([now.hour, now.minute, now.second])
        d = bytes([now.year % 100, now.isoweekday(), now.month, now.day])
        self._screen_transaction([(self._screen_packet(PKT_SET_TIME, t), None)])
        self._screen_transaction([(self._screen_packet(PKT_SET_DATE, d), None)])

    def upload_image(self, pixels, size=SCREEN_DEFAULT):
        w, h = size
        if len(pixels) != w * h * 2:
            raise ValueError(f"expected {w*h*2} bytes of RGB565")
        p = self._screen_packet
        # GUI event opens the transaction on 0x40; the device needs time to
        # erase the image slot before the pixel stream arrives.
        self._screen_send(p(PKT_GUI_EVENT, bytes([GUI_IMAGE_UPLOAD])), None, False)
        time.sleep(0.35)
        self._screen_send(p(PKT_IMAGE_DATA, b"", declared=len(pixels)), None, True)
        time.sleep(0.05)
        self._screen_send(pixels, None, True)
        time.sleep(0.5)
        self._exchange(SCREEN_FINISH, bytes(CAPACITY), 0, timeout=6.0)
        time.sleep(0.3)

    def upload_gif(self, frames, slot="user", fps=12, size=SCREEN_DEFAULT):
        w, h = size
        frame_bytes = w * h * 2
        for i, f in enumerate(frames):
            if len(f) != frame_bytes:
                raise ValueError(f"frame {i} is {len(f)} bytes, need {frame_bytes}")
        code = SLOTS[slot]
        p = self._screen_packet
        hdr = p(PKT_GIF_DATA, b"", declared=frame_bytes)
        steps = [
            (p(PKT_GIF_FRAMES, bytes([code, 0])), None),
            (p(PKT_GIF_FPS, bytes([code, 0])), None),
        ]
        for i, f in enumerate(frames):
            steps.append((p(PKT_GUI_EVENT, bytes([GUI_GIF_FRAME, code, i])), None))
            steps.append((hdr, None))
            off = 0
            while off < len(f):
                steps.append((f[off:off + 1024], hdr))
                off += 1024
        steps.append((p(PKT_GIF_FRAMES, bytes([code, len(frames)])), None))
        steps.append((p(PKT_GIF_FPS, bytes([code, fps])), None))
        self._screen_transaction(steps)

    # ---- per-key custom light (0x19 read / 0x1a write) ----
    def _read_blob(self, cmd, total):
        out = bytearray()
        addr = 0
        while addr < total:
            n = min(CAPACITY, total - addr)
            out += self._exchange(cmd, bytes(n), addr)
            addr += n
        return out

    def _write_blob(self, cmd, blob):
        addr = 0
        while addr < len(blob):
            chunk = bytes(blob[addr:addr + CAPACITY])
            self._exchange(cmd, chunk, addr)
            addr += CAPACITY

    def read_matrix(self):
        return self._read_blob(0x19, LIGHT_COUNT * LIGHT_ENTRY)

    def write_matrix(self, matrix):
        if len(matrix) != LIGHT_COUNT * LIGHT_ENTRY:
            raise ValueError(f"matrix must be {LIGHT_COUNT * LIGHT_ENTRY} bytes")
        self._write_blob(0x1A, matrix)

    # ---- realtime preview (0x1d / 0x1e) ----
    def sync_frame(self, frame):
        if len(frame) != PREVIEW_COUNT * PREVIEW_ENTRY:
            raise ValueError(f"preview frame must be {PREVIEW_COUNT * PREVIEW_ENTRY} bytes")
        self._write_blob(0x1D, frame)

    def stop_sync(self):
        self._exchange(0x1E, b"", 0)


def parse_color(s):
    s = s.lstrip("#")
    if len(s) != 6:
        raise argparse.ArgumentTypeError("colour must be RRGGBB")
    return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--device", help="hidraw node (default: autodetect)")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("status", help="show current lighting state")

    p = sub.add_parser("light", help="set lighting")
    p.add_argument("--mode", choices=sorted(MODES), help="effect")
    p.add_argument("--level", type=int, choices=range(5), help="brightness 0-4")
    p.add_argument("--speed", type=int, choices=range(5), help="speed 0-4")
    p.add_argument("--color", type=parse_color, help="RRGGBB")
    p.add_argument("--rgb", type=int, choices=(0, 1), help="use colour table")
    p.add_argument("--direction", choices=sorted(DIRECTIONS), help="effect direction")

    p = sub.add_parser("sync", help="push one realtime frame (84 LEDs)")
    p.add_argument("--all", type=parse_color, help="fill with RRGGBB")
    p.add_argument("--load", metavar="FILE", help=f"{PREVIEW_COUNT*PREVIEW_ENTRY}-byte frame")

    sub.add_parser("stop", help="stop realtime sync")

    p = sub.add_parser("demo", help="stream a rainbow animation")
    p.add_argument("--seconds", type=float, default=10)
    p.add_argument("--fps", type=float, default=20)
    p.add_argument("--level", type=int, choices=range(5), default=4)

    p = sub.add_parser("perkey", help="per-key custom colours (mode 17)")
    p.add_argument("--all", type=parse_color, help="set every key to RRGGBB")
    p.add_argument("--set", action="append", default=[], metavar="N=RRGGBB",
                   help="set key index N (repeatable)")
    p.add_argument("--level", type=int, choices=range(5))
    p.add_argument("--save", metavar="FILE", help="write current matrix to FILE")
    p.add_argument("--load", metavar="FILE", help="load matrix from FILE")
    p.add_argument("--no-mode", action="store_true", help="don't switch to mode 17")

    p = sub.add_parser("image", help="upload a still image to the TFT")
    p.add_argument("file")
    p.add_argument("--width", type=int, default=SCREEN_DEFAULT[0])
    p.add_argument("--height", type=int, default=SCREEN_DEFAULT[1])
    p.add_argument("--no-switch", action="store_true")

    p = sub.add_parser("gif", help="upload an animated GIF to the TFT")
    p.add_argument("file")
    p.add_argument("--slot", choices=sorted(SLOTS), default="user")
    p.add_argument("--fps", type=int, default=12)
    p.add_argument("--width", type=int, default=SCREEN_DEFAULT[0])
    p.add_argument("--height", type=int, default=SCREEN_DEFAULT[1])

    p = sub.add_parser("screen", help="switch TFT page / sync clock")
    p.add_argument("page", choices=["home", "image", "gif", "time"])

    p = sub.add_parser("scan", help="step through every mode id so you can identify them")
    p.add_argument("--delay", type=float, default=2.5)
    p.add_argument("--color", type=parse_color, default=(0, 255, 0))

    p = sub.add_parser("off", help="turn the backlight off")
    p = sub.add_parser("timer", help="set the USB LED auto-off timer")
    p.add_argument("seconds", type=int)

    args = ap.parse_args()
    path = args.device or find_device()
    if not path:
        sys.exit("device not found (VID 379a PID 0920, interface 1)")
    dev = Phantom(path)
    try:
        if args.cmd == "status":
            a = dev.status()
            print(f"device       : {path}")
            print(f"mode         : {a[1]} ({MODES_BY_ID.get(a[1], '?')})")
            print(f"brightness   : {a[2]}")
            print(f"speed        : {a[3]}")
            print(f"direction    : {a[4]}")
            print(f"use colour   : {a[5]}")
            print(f"colour       : {a[6]:02x}{a[7]:02x}{a[8]:02x}")
            print(f"usb led timer: {a[23] | (a[24] << 8)}")
        elif args.cmd == "light":
            mode = MODES[args.mode] if args.mode else None
            dev.set_light(mode=mode, level=args.level, speed=args.speed,
                          color=args.color, rgb=args.rgb,
                          direction=DIRECTIONS[args.direction] if args.direction else None)
            a = dev.status()
            print(f"mode={a[1]} ({MODES_BY_ID.get(a[1], '?')}) level={a[2]} "
                  f"speed={a[3]} colour={a[6]:02x}{a[7]:02x}{a[8]:02x}")
        elif args.cmd == "sync":
            if args.load:
                with open(args.load, "rb") as f:
                    frame = f.read()
            elif args.all:
                frame = b"".join(bytes(args.all) for _ in range(PREVIEW_COUNT))
            else:
                sys.exit("give --all RRGGBB or --load FILE")
            dev.sync_frame(frame)
            print("frame sent")
        elif args.cmd == "stop":
            dev.stop_sync()
            print("sync stopped")
        elif args.cmd == "demo":
            import colorsys
            dev.set_light(mode=MODES["custom"], level=args.level)
            n = PREVIEW_COUNT
            t0 = time.time()
            i = 0
            try:
                while time.time() - t0 < args.seconds:
                    frame = bytearray(n * PREVIEW_ENTRY)
                    for k in range(n):
                        hue = ((k / n) + (i / 40.0)) % 1.0
                        r, g, b = colorsys.hsv_to_rgb(hue, 1.0, 1.0)
                        frame[k*3:k*3+3] = bytes((int(r*255), int(g*255), int(b*255)))
                    dev.sync_frame(bytes(frame))
                    i += 1
                    time.sleep(1.0 / args.fps)
            except KeyboardInterrupt:
                pass
            finally:
                dev.stop_sync()
            print(f"streamed {i} frames")
        elif args.cmd == "perkey":
            if args.load:
                with open(args.load, "rb") as f:
                    mat = bytearray(f.read())
            else:
                mat = bytearray(dev.read_matrix())
            if len(mat) != LIGHT_COUNT * LIGHT_ENTRY:
                sys.exit(f"matrix file must be {LIGHT_COUNT * LIGHT_ENTRY} bytes")
            if args.all:
                for i in range(LIGHT_COUNT):
                    mat[i * 3:i * 3 + 3] = bytes(args.all)
            for item in args.set:
                try:
                    idx, col = item.split("=", 1)
                    idx = int(idx, 0)
                    rgb = parse_color(col)
                except Exception:
                    sys.exit(f"bad --set {item!r}, expected N=RRGGBB")
                if not 0 <= idx < LIGHT_COUNT:
                    sys.exit(f"key index {idx} out of range 0..{LIGHT_COUNT - 1}")
                mat[idx * 3:idx * 3 + 3] = bytes(rgb)
            dev.write_matrix(bytes(mat))
            if args.save:
                with open(args.save, "wb") as f:
                    f.write(mat)
                print(f"saved matrix to {args.save}")
            if not args.no_mode:
                dev.set_light(mode=MODES["custom"], level=args.level)
            a = dev.status()
            print(f"mode={a[1]} ({MODES_BY_ID.get(a[1], '?')}) level={a[2]}")
        elif args.cmd == "image":
            import subprocess
            w, h = args.width, args.height
            raw = subprocess.check_output(
                ["magick", args.file, "-resize", f"{w}x{h}!", "-depth", "8",
                 "rgb:-"])
            px = bytearray()
            for i in range(0, len(raw) - 2, 3):
                r, g, b = raw[i], raw[i + 1], raw[i + 2]
                v = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
                px += bytes([v >> 8, v & 0xFF])
            dev.upload_image(bytes(px), (w, h))
            if not args.no_switch:
                dev.screen_switch(PKT_SWITCH_IMAGE)
            print(f"uploaded {w}x{h} image")
        elif args.cmd == "gif":
            import subprocess
            w, h = args.width, args.height
            raw = subprocess.check_output(
                ["magick", args.file, "-coalesce", "-resize", f"{w}x{h}!",
                 "-depth", "8", "rgb:-"])
            n = subprocess.check_output(
                ["identify", args.file]).decode().count("\n")
            fb = w * h * 3
            frames = []
            for i in range(n):
                chunk = raw[i * fb:(i + 1) * fb]
                if len(chunk) < fb:
                    break
                px = bytearray()
                for j in range(0, fb, 3):
                    r, g, b = chunk[j], chunk[j + 1], chunk[j + 2]
                    v = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
                    px += bytes([v >> 8, v & 0xFF])
                frames.append(bytes(px))
            dev.upload_gif(frames, slot=args.slot, fps=args.fps, size=(w, h))
            dev.screen_switch(PKT_SWITCH_GIF)
            print(f"uploaded {len(frames)} frames at {args.fps} fps")
        elif args.cmd == "screen":
            if args.page == "time":
                dev.screen_time()
            else:
                dev.screen_switch({"home": PKT_HOME, "image": PKT_SWITCH_IMAGE,
                                   "gif": PKT_SWITCH_GIF}[args.page])
            print("ok")
        elif args.cmd == "scan":
            for mode in range(19):
                dev.set_light(mode=mode, level=4, speed=1, color=args.color)
                print(f"mode {mode:2d}  ({MODES_BY_ID.get(mode, '?')})", flush=True)
                time.sleep(args.delay)
            dev.set_light(mode=MODES["static"], level=4, speed=1, color=args.color)
            print("restored static")
        elif args.cmd == "off":
            dev.set_light(mode=MODES["off"])
            print("backlight off")
        elif args.cmd == "timer":
            area = dev.read_area()
            area[23] = args.seconds & 0xFF
            area[24] = (args.seconds >> 8) & 0xFF
            dev.write_area(area)
            print(f"usb led auto-off timer = {args.seconds}s")
    finally:
        dev.close()


if __name__ == "__main__":
    main()
