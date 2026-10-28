# Running the vendor app under Wine

## The problem

Wine creates `/dev/hidraw*` nodes for USB HID devices, but it does not register
HID device interfaces with Windows' SetupAPI. `hidapi` (and therefore the
vendor application) enumerates through SetupAPI and sees nothing. QEMU with USB
passthrough would work but is heavy; a userspace proxy is enough.

The solution is a replacement `hidapi.dll` that ignores Wine's HID stack and
forwards to a Linux daemon that owns the real hidraw node.

```
PhantomS Software.exe
        |
        | hidapi.dll  (our 32-bit PE shim)
        |   hid_enumerate / hid_open_path / hid_write / hid_read ...
        v
127.0.0.1:47820  (line protocol)
        |
        v
hidrawd.py  (owns /dev/hidrawN)
        |
        v
keyboard
```

## Components

| file | role |
|---|---|
| `bridge/shim.c` | `hidapi.dll` proxy, compiled as 32-bit PE |
| `bridge/hidrawd.py` | hidraw owner and TCP server |
| `bridge/decode.py` | decode captured frames from the log |
| `app.sh` | start/stop daemon + app, with path and timer handling |

### Exported API

The shim exports the subset the application imports:

```
hid_enumerate          hid_free_enumeration
hid_open_path          hid_open
hid_close              hid_write
hid_read               hid_read_timeout
hid_set_nonblocking    hid_error
hid_init               hid_exit
```

Enumeration fabricates one device entry for `379a:0920`, interface 1, usage
page `0xFF87`, usage `0x20`, so the application's own filtering matches.

### Wire protocol

Little-endian, over loopback TCP:

```
request:  u8 op, u16 a, u16 b
          op=1 WRITE  a=len, then len bytes
          op=2 READ   a=timeout_ms
reply:    u8 status, u16 len, then len bytes
```

Each opened handle gets its own TCP connection. The earlier design used one
socket plus a global lock, which deadlocked the UI: a blocking read on one
handle stalled writes on another. A per-handle socket pool fixed that.

Replies are queued in a global `reply_q` and handed to whichever connection is
waiting. A read first takes a queued reply, otherwise waits on the hidraw fd
outside the lock.

### Function-area expansion

This is the most important workaround. The vendor application writes only the
first 56 bytes of the 128-byte function area:

```
0x15, addr=0, len=56
```

The firmware only commits the area when all 128 bytes are written, and still
replies `0x55` to the partial write, so the application believes it succeeded
while nothing changes. The daemon detects that exact frame and:

1. reads the current 128-byte area from the device;
2. replaces bytes 0-55 with the application's payload;
3. for timer offsets 15/17/19/21/23, keeps the cached value when the
   application sends a zero pair (the UI displays timers as 0 and would
   otherwise wipe them);
4. writes all three chunks (0, 56, 112) and queues only the first reply, so
   the application still sees exactly one reply for its one write;
5. updates its cache.

Expansion runs on a background thread: the application gets an immediate
success response, so rapid UI clicks do not block. The lock serialises
expansions, and the area cache prevents a partially-read area from becoming
the new baseline.

### Device verification

The daemon refuses to open a hidraw node unless its sysfs uevent contains
`0000379a:00000920`. This avoids grabbing the wrong node when several HID
devices are present.

## Building

The application is a 32-bit PE, so the DLL must be 32-bit:

```bash
cd bridge
make build
# equivalent to:
i686-w64-mingw32-gcc -shared -O2 -o hidapi.dll shim.c -lws2_32
```

Check exports if the app fails to load it:

```bash
i686-w64-mingw32-objdump -p hidapi.dll | grep hid_
```

Names must be undecorated (`hid_write`, not `hid_write@12`). If they are
decorated, add `-Wl,--kill-at` or make the declarations `__cdecl`.

## Installing and running

The vendor directory is not committed. By default `app.sh` looks for
`./program_files_x86`, then `../program_files_x86`; override with
`PHANTOM_APP_DIR`.

```bash
# one-time: keep the original
cp program_files_x86/hidapi.dll program_files_x86/hidapi_real.dll
cp bridge/hidapi.dll program_files_x86/hidapi.dll

./app.sh          # daemon + app
./app.sh status
./app.sh stop
./app.sh log      # follow bridge/hidrawd.log
```

`app.sh` also accepts `./app.sh daemon` and `./app.sh app`.

Wine prefix defaults to `./wine_prefix` (or `WINEPREFIX`). The daemon listens
on `127.0.0.1:47820`.

## Logs

- `bridge/hidrawd.log` -- every frame, hex encoded, with timestamps
- `bridge/daemon.out` -- daemon stdout/stderr
- `hidapi_shim.log` -- shim-side log, written next to the application

`bridge/decode.py` parses the log into human-readable frames (report, command,
address, checksum, result).

## Caveats

- This path is only for Wine. On real Windows the vendor `hidapi.dll` works;
  do not ship the shim there.
- The app is a PE32 executable; a 64-bit DLL will not load.
- Wine caches loaded DLLs per process: stop the app, replace the DLL, restart.
- The daemon must be restarted after a replug if the hidraw node changes. It
  verifies VID/PID by sysfs, but it still holds the old node path.
- The shim is a compatibility layer, not a general hidapi implementation:
  feature reports, serial numbers and most metadata are stubbed.
