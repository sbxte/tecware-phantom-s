/* hidapi.dll proxy for running PhantomS Software.exe under Wine.
 *
 * Wine creates /dev/hidraw nodes but never registers HID device interfaces
 * with SetupAPI, so the real hidapi (and therefore the app) sees nothing.
 * This DLL ignores Wine's HID stack and forwards to hidrawd.py over TCP.
 *
 * Each opened handle gets its own connection, so a blocking read on one
 * handle cannot stall writes on another (the earlier single-socket +
 * global-lock design froze the app's UI).
 *
 * The exported symbol names and the hid_device_info layout follow the public
 * hidapi ABI (hidapi is BSD-3-Clause). Everything below is an independent
 * implementation; no hidapi source is copied.
 *
 * Build:
 *   i686-w64-mingw32-gcc -shared -O2 -o hidapi.dll shim.c -lws2_32
 */
#include <winsock2.h>
#include <windows.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define DAEMON_ADDR "127.0.0.1"
#define DAEMON_PORT 47820
#define LOG_PATH    "hidapi_shim.log"

#define EXPORT __declspec(dllexport)

typedef struct hid_device_info {
    char *path;
    unsigned short vendor_id;
    unsigned short product_id;
    wchar_t *serial_number;
    unsigned short release_number;
    wchar_t *manufacturer_string;
    wchar_t *product_string;
    unsigned short usage_page;
    unsigned short usage;
    int interface_number;
    struct hid_device_info *next;
    int interface_class;
} hid_device_info;

typedef struct hid_device {
    SOCKET sock;
    int pool;
    char path[160];
} hid_device;

static HANDLE g_log = INVALID_HANDLE_VALUE;

static void logline(const char *fmt, ...)
{
    char buf[4096];
    va_list ap;
    int n;
    SYSTEMTIME st;

    GetLocalTime(&st);
    n = snprintf(buf, sizeof buf, "%02d:%02d:%02d.%03d ",
                 st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    if (n < 0)
        n = 0;
    va_start(ap, fmt);
    n += vsnprintf(buf + n, sizeof buf - n, fmt, ap);
    va_end(ap);
    if (n <= 0)
        return;

    if (g_log == INVALID_HANDLE_VALUE)
        g_log = CreateFileA(LOG_PATH, FILE_APPEND_DATA,
                            FILE_SHARE_READ | FILE_SHARE_WRITE, NULL,
                            OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (g_log != INVALID_HANDLE_VALUE) {
        DWORD written;
        WriteFile(g_log, buf, (DWORD)n, &written, NULL);
        WriteFile(g_log, "\r\n", 2, &written, NULL);
    }
}

static void loghex(const char *tag, const unsigned char *data, int len)
{
    char line[4096];
    int i, off = 0;
    off += snprintf(line + off, sizeof line - off, "%s len=%d ", tag, len);
    for (i = 0; i < len && off < (int)sizeof line - 4; i++)
        off += snprintf(line + off, sizeof line - off, "%02x", data[i]);
    logline("%s", line);
}

static int send_all(SOCKET s, const void *buf, int len)
{
    const char *p = (const char *)buf;
    while (len > 0) {
        int n = send(s, p, len, 0);
        if (n <= 0)
            return -1;
        p += n;
        len -= n;
    }
    return 0;
}

static int recv_all(SOCKET s, void *buf, int len)
{
    char *p = (char *)buf;
    while (len > 0) {
        int n = recv(s, p, len, 0);
        if (n <= 0)
            return -1;
        p += n;
        len -= n;
    }
    return 0;
}

static SOCKET connect_daemon(void)
{
    WSADATA wsa;
    SOCKET s;
    struct sockaddr_in addr;

    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0)
        return INVALID_SOCKET;
    s = socket(AF_INET, SOCK_STREAM, 0);
    if (s == INVALID_SOCKET)
        return INVALID_SOCKET;
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_port = htons(DAEMON_PORT);
    addr.sin_addr.s_addr = inet_addr(DAEMON_ADDR);
    if (connect(s, (struct sockaddr *)&addr, sizeof addr) != 0) {
        logline("connect to daemon failed: %d", WSAGetLastError());
        closesocket(s);
        return INVALID_SOCKET;
    }
    return s;
}

#define POOL_MAX 8
static struct {
    char path[160];
    SOCKET sock;
    CRITICAL_SECTION cs;
} g_pool[POOL_MAX];
static int g_pool_n = 0;

static int pool_index(const char *path)
{
    int i;
    for (i = 0; i < g_pool_n; i++)
        if (strcmp(g_pool[i].path, path) == 0)
            return i;
    if (g_pool_n >= POOL_MAX)
        return -1;
    g_pool[g_pool_n].sock = connect_daemon();
    if (g_pool[g_pool_n].sock == INVALID_SOCKET)
        return -1;
    strncpy(g_pool[g_pool_n].path, path, sizeof g_pool[0].path - 1);
    InitializeCriticalSection(&g_pool[g_pool_n].cs);
    g_pool_n++;
    logline("pool: new connection for %s", path);
    return g_pool_n - 1;
}

static SOCKET dev_sock(hid_device *dev)
{
    int i;
    if (dev == NULL)
        return INVALID_SOCKET;
    i = pool_index(dev->path);
    if (i < 0)
        return INVALID_SOCKET;
    dev->sock = g_pool[i].sock;
    dev->pool = i;
    return dev->sock;
}

static hid_device_info *make_entry(unsigned short vid, unsigned short pid,
                                   const char *path, unsigned short page,
                                   unsigned short usage, int iface)
{
    hid_device_info *e = (hid_device_info *)calloc(1, sizeof *e);
    if (!e)
        return NULL;
    e->path = _strdup(path);
    e->vendor_id = vid;
    e->product_id = pid;
    e->release_number = 0x0113;
    e->manufacturer_string = _wcsdup(L"I-CHIP");
    e->product_string = _wcsdup(L"Phantom S");
    e->serial_number = _wcsdup(L"1234");
    e->usage_page = page;
    e->usage = usage;
    e->interface_number = iface;
    e->interface_class = 3;
    e->next = NULL;
    return e;
}

static const unsigned short g_pids[] = { 0x0920, 0x0921 };
static const struct { unsigned short page, usage; const char *col; } g_cols[] = {
    { 0xff87, 0x20, "col01" },
    { 0xff88, 0x21, "col02" },
    { 0xff00, 0x01, "col03" },
};

EXPORT hid_device_info *hid_enumerate(unsigned short vid, unsigned short pid)
{
    hid_device_info *head = NULL, **tail = &head;
    char path[128];
    int i, j;

    logline("hid_enumerate(%04x,%04x)", vid, pid);
    if (vid && vid != 0x379a)
        return NULL;

    for (i = 0; i < (int)(sizeof g_pids / sizeof g_pids[0]); i++) {
        unsigned short p = g_pids[i];
        if (pid && pid != p)
            continue;
        for (j = 0; j < (int)(sizeof g_cols / sizeof g_cols[0]); j++) {
            hid_device_info *e;
            snprintf(path, sizeof path,
                     "\\\\?\\hid#vid_379a&pid_%04x&mi_01&%s#phantom",
                     p, g_cols[j].col);
            e = make_entry(0x379a, p, path, g_cols[j].page, g_cols[j].usage, 1);
            if (!e)
                continue;
            *tail = e;
            tail = &e->next;
        }
    }
    return head;
}

EXPORT void hid_free_enumeration(hid_device_info *list)
{
    while (list) {
        hid_device_info *next = list->next;
        free(list->path);
        free(list->serial_number);
        free(list->manufacturer_string);
        free(list->product_string);
        free(list);
        list = next;
    }
}

EXPORT hid_device *hid_open_path(const char *path)
{
    hid_device *dev = (hid_device *)calloc(1, sizeof *dev);
    if (!dev)
        return NULL;
    dev->sock = INVALID_SOCKET;
    if (path)
        strncpy(dev->path, path, sizeof dev->path - 1);
    logline("hid_open_path(%s)", path ? path : "(null)");
    return dev;
}

EXPORT hid_device *hid_open(unsigned short vid, unsigned short pid, const wchar_t *serial)
{
    hid_device *dev;
    if (vid != 0x379a || (pid != 0x0920 && pid != 0x0921))
        return NULL;
    dev = (hid_device *)calloc(1, sizeof *dev);
    if (!dev)
        return NULL;
    dev->sock = INVALID_SOCKET;
    strcpy(dev->path, "hid_open");
    return dev;
}

EXPORT void hid_close(hid_device *dev)
{
    if (!dev)
        return;
    logline("hid_close(%p)", (void *)dev);
    free(dev);
}

EXPORT int hid_write(hid_device *dev, const unsigned char *data, size_t len)
{
    SOCKET s;
    unsigned char hdr[5];
    unsigned char reply[3];
    int ok;

    if (!dev || !data)
        return -1;
    s = dev_sock(dev);
    if (s == INVALID_SOCKET)
        return -1;

    hdr[0] = 1;
    hdr[1] = (unsigned char)(len & 0xff);
    hdr[2] = (unsigned char)((len >> 8) & 0xff);
    hdr[3] = 0;
    hdr[4] = 0;
    EnterCriticalSection(&g_pool[dev->pool].cs);
    ok = send_all(s, hdr, 5) || send_all(s, data, (int)len) || recv_all(s, reply, 3);
    LeaveCriticalSection(&g_pool[dev->pool].cs);
    if (ok) {
        logline("hid_write failed (%s)", dev->path);
        closesocket(s);
        dev->sock = INVALID_SOCKET;
        return -1;
    }
    loghex("WRITE", data, (int)len);
    return reply[0] == 0 ? (int)len : -1;
}

EXPORT int hid_read_timeout(hid_device *dev, unsigned char *data, size_t len, int ms)
{
    SOCKET s;
    unsigned char hdr[5];
    unsigned char reply[3];
    unsigned short n;

    if (!dev || !data || len == 0)
        return -1;
    if (ms < 0)
        ms = 0;
    s = dev_sock(dev);
    if (s == INVALID_SOCKET)
        return -1;

    hdr[0] = 2;
    hdr[1] = (unsigned char)(ms & 0xff);
    hdr[2] = (unsigned char)((ms >> 8) & 0xff);
    hdr[3] = 0;
    hdr[4] = 0;
    EnterCriticalSection(&g_pool[dev->pool].cs);
    if (send_all(s, hdr, 5) || recv_all(s, reply, 3)) {
        LeaveCriticalSection(&g_pool[dev->pool].cs);
        logline("hid_read failed (%s)", dev->path);
        closesocket(s);
        dev->sock = INVALID_SOCKET;
        return -1;
    }
    n = (unsigned short)(reply[1] | (reply[2] << 8));
    if (n > 0) {
        if (n > len)
            n = (unsigned short)len;
        if (recv_all(s, data, n)) {
            LeaveCriticalSection(&g_pool[dev->pool].cs);
            closesocket(s);
            dev->sock = INVALID_SOCKET;
            return -1;
        }
    }
    LeaveCriticalSection(&g_pool[dev->pool].cs);
    return n;
}

EXPORT int hid_read(hid_device *dev, unsigned char *data, size_t len)
{
    return hid_read_timeout(dev, data, len, 1000);
}

EXPORT int hid_set_nonblocking(hid_device *dev, int nonblock)
{
    (void)dev;
    (void)nonblock;
    return 0;
}

EXPORT const wchar_t *hid_error(hid_device *dev)
{
    (void)dev;
    return L"";
}

EXPORT int hid_init(void) { return 0; }

EXPORT int hid_exit(void)
{
    WSACleanup();
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved)
{
    (void)inst;
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH)
        DisableThreadLibraryCalls(inst);
    return TRUE;
}
