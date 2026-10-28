#!/usr/bin/env python3
"""Owns a hidraw node and exposes it over TCP for the Wine-side hidapi shim.

Protocol (little-endian):
  request : u8 op, u16 a, u16 b
            op=1 WRITE  a=len,  b unused, then `len` bytes
            op=2 READ   a=timeout_ms, b unused
  reply   : u8 status, u16 len, then `len` bytes

Usage: hidrawd.py [device] [port] [logfile]
"""
import collections
import os
import select
import socket
import struct
import sys
import threading
import time

DEV = sys.argv[1] if len(sys.argv) > 1 else "/dev/hidraw1"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 47820
LOGFILE = sys.argv[3] if len(sys.argv) > 3 else "hidrawd.log"

lock = threading.Lock()
logf = open(LOGFILE, "a", buffering=1)
area_cache = None              # last known 128-byte function area
reply_q = collections.deque()   # device replies not yet delivered to a client


def log(direction, data):
    logf.write(f"{time.time():.6f} {direction} len={len(data):3d} {data.hex()}\n")


def recv_exact(conn, n):
    buf = b""
    while len(buf) < n:
        chunk = conn.recv(n - len(buf))
        if not chunk:
            raise EOFError
        buf += chunk
    return buf


def build(cmd, payload, addr):
    b = bytearray(64)
    b[0] = 6
    b[1] = cmd
    b[2] = addr & 0xFF
    b[3] = (addr >> 8) & 0xFF
    b[4] = len(payload)
    b[8:8 + len(payload)] = payload
    c = (sum(b[1:5]) + sum(b[7:])) & 0xFFFF
    b[5] = c & 0xFF
    b[6] = (c >> 8) & 0xFF
    return bytes(b)


def xfer_locked(fd, frame, timeout=0.6):
    os.write(fd, frame)
    deadline = time.time() + timeout
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], max(0.0, deadline - time.time()))
        if not r:
            break
        d = os.read(fd, 256)
        if len(d) > 1 and d[1] == frame[1]:
            return d
    return None


def read_area_locked(fd):
    area = bytearray()
    for addr, n in ((0, 56), (56, 56), (112, 16)):
        r = xfer_locked(fd, build(0x14, bytes(n), addr))
        if r is None:
            return None
        area += r[8:8 + r[4]]
    return area


def is_area_write(data):
    return (len(data) >= 8 and data[0] == 6 and data[1] == 0x15
            and (data[2] | (data[3] << 8)) == 0 and data[4] == 56)


def do_expand(fd, data):
    with lock:
        try:
            expand_area_write(fd, data)
        except Exception as e:
            logf.write(f"expand error: {e!r}\n")


def expand_area_write(fd, data):
    global area_cache
    """The app writes only the first 56 bytes of the 128-byte function area.
    The firmware only commits the full area, so build the other chunks from
    the device's current state and send all three."""
    if not is_area_write(data):
        return None
    if area_cache is None:
        area_cache = read_area_locked(fd)
        if area_cache is None or len(area_cache) != 128:
            area_cache = None
            return None
    cached = area_cache
    area = bytearray(cached)
    area[0:56] = data[8:64]
    # The app's UI shows the timers as 0 and sends zeros; keep our values
    # for any timer field the app is trying to zero out.
    for off in (15, 17, 19, 21, 23):
        if area[off] == 0 and area[off + 1] == 0:
            area[off:off + 2] = cached[off:off + 2]
    first = True
    for a in (0, 56, 112):
        r = xfer_locked(fd, build(0x15, bytes(area[a:a + 56]), a), timeout=0.2)
        # the app issued one write and expects one reply; keep only that one
        if r is not None and first:
            reply_q.append(r)
            first = False
    area_cache = area
    return 64


def handle(conn, fd):
    try:
        while True:
            hdr = recv_exact(conn, 5)
            op = hdr[0]
            a, b = struct.unpack("<HH", hdr[1:5])

            if op == 1:  # WRITE
                data = recv_exact(conn, a)
                log("TX", data)
                if is_area_write(data):
                    # ack immediately; the function-area expansion runs in the
                    # background so rapid UI clicks don't block the caller
                    threading.Thread(target=do_expand, args=(fd, bytes(data)),
                                     daemon=True).start()
                    n, status = len(data), 0
                else:
                    with lock:
                        try:
                            n = os.write(fd, data)
                            r = None
                            ready, _, _ = select.select([fd], [], [], 0.2)
                            if ready:
                                r = os.read(fd, 256)
                            if r:
                                reply_q.append(r)
                            status = 0
                        except OSError as e:
                            n, status = 0, 1
                            logf.write(f"write error: {e}\n")
                conn.sendall(bytes([status]) + struct.pack("<H", n))

            elif op == 2:  # READ
                data = b""
                with lock:
                    if reply_q:
                        data = reply_q.popleft()
                if not data:
                    # wait outside the lock so a slow read can't block writes
                    ready, _, _ = select.select([fd], [], [], a / 1000.0)
                    if ready:
                        with lock:
                            if reply_q:
                                data = reply_q.popleft()
                            else:
                                try:
                                    data = os.read(fd, 256)
                                except OSError:
                                    data = b""
                if data:
                    log("RX", data)
                conn.sendall(bytes([0]) + struct.pack("<H", len(data)) + data)

            else:
                conn.sendall(bytes([1]) + struct.pack("<H", 0))
    except (EOFError, ConnectionError, OSError):
        pass
    except Exception as e:  # never let one client kill its thread silently
        logf.write(f"handler error: {e!r}\n")
    finally:
        conn.close()


def verify_device(path):
    """Refuse to open anything that isn't the Phantom S vendor interface."""
    name = os.path.basename(os.path.realpath(path))
    uevent = f"/sys/class/hidraw/{name}/device/uevent"
    try:
        with open(uevent) as f:
            text = f.read()
    except OSError:
        return
    if "0000379a:00000920" not in text.lower():
        sys.exit(f"hidrawd: refusing {path}: not the Phantom S\n{text}")


def main():
    verify_device(DEV)
    fd = os.open(DEV, os.O_RDWR)
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", PORT))
    srv.listen(8)
    print(f"hidrawd: {DEV} on 127.0.0.1:{PORT}, log={LOGFILE}", flush=True)
    while True:
        conn, _ = srv.accept()
        threading.Thread(target=handle, args=(conn, fd), daemon=True).start()


if __name__ == "__main__":
    main()
