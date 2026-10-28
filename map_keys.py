#!/usr/bin/env python3
"""Point-and-click mapper: assign an X/Y position in a photo to each key.

    python3 map_keys.py            # serves http://127.0.0.1:8765 and opens it

Load a photo of your keyboard, press "Begin mapping", then click each key
on the image as its name is shown. Coordinates are stored normalised (0..1)
in keymap_xy.json after every Save.

Keys are taken from mapping_custom.json (the LED index -> key names you
probed), deduplicated and with the odd entries filtered out.
"""
import http.server
import json
import os
import webbrowser

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "keymap_xy.json")
PORT = 8765

BAD = ("red somehow", "faint red", "[]")


def load_keys():
    path = os.path.join(HERE, "mapping_custom.json")
    if not os.path.exists(path):
        raise SystemExit("mapping_custom.json not found; run the probe first")
    mapping = json.load(open(path))
    seen, keys = set(), []
    for idx in sorted(mapping, key=int):
        v = mapping[idx]
        name = v[0] if isinstance(v, list) and v else (v if not isinstance(v, list) else None)
        if not name or any(b in name.lower() for b in BAD):
            continue
        if name in seen:
            continue
        seen.add(name)
        keys.append(name)
    return keys


PAGE = r"""<!doctype html>
<html><head><meta charset="utf-8"><title>Keyboard key mapper</title>
<style>
 body{font-family:system-ui,sans-serif;margin:16px;background:#14161a;color:#e6e6e6}
 canvas{border:1px solid #3a3f47;cursor:crosshair;display:block;margin-top:10px;background:#000}
 button,input{font:inherit}
 button{margin:2px;padding:6px 12px;background:#2a2f36;color:#e6e6e6;border:1px solid #444;border-radius:4px;cursor:pointer}
 button:hover{background:#353b44}
 #status{font-size:1.15em;margin:10px 0;padding:8px;background:#1d2127;border-radius:4px}
 #info{color:#8b949e;margin-left:8px}
</style></head>
<body>
<h2 style="margin:0 0 10px">Keyboard key mapper</h2>
<input type="file" id="file" accept="image/*">
<button id="begin">Begin mapping</button>
<button id="skip">Skip</button>
<button id="undo">Undo</button>
<button id="save">Save</button>
<span id="info"></span>
<div id="status">Load an image, then press Begin.</div>
<canvas id="c"></canvas>
<script>
const KEYS = __KEYS__;
const MAP = __EXISTING__;
let img = null, cur = -1, history = [];
const c = document.getElementById('c'), ctx = c.getContext('2d');
const statusEl = document.getElementById('status');

function fit(){
  if(!img) return;
  const maxW = Math.min(window.innerWidth - 40, 1500);
  const maxH = Math.min(window.innerHeight - 220, 950);
  const s = Math.min(maxW / img.naturalWidth, maxH / img.naturalHeight, 1);
  c.width = Math.round(img.naturalWidth * s);
  c.height = Math.round(img.naturalHeight * s);
  draw();
}
function draw(){
  ctx.clearRect(0, 0, c.width, c.height);
  if(img) ctx.drawImage(img, 0, 0, c.width, c.height);
  ctx.font = '13px sans-serif';
  for(const k of KEYS){
    const p = MAP[k]; if(!p) continue;
    const x = p[0]*c.width, y = p[1]*c.height;
    ctx.beginPath(); ctx.arc(x, y, 4, 0, 7); ctx.fillStyle = '#ffdd00'; ctx.fill();
    ctx.fillStyle = '#ffdd00'; ctx.fillText(k, x + 7, y + 4);
  }
  statusEl.textContent = cur >= 0
    ? `Click on "${KEYS[cur]}"   (${Object.keys(MAP).length}/${KEYS.length} mapped)`
    : `All done or not started.  ${Object.keys(MAP).length}/${KEYS.length} mapped.`;
}
function next(){
  for(let step = 1; step <= KEYS.length; step++){
    const i = ((cur + step) % KEYS.length + KEYS.length) % KEYS.length;
    if(!(KEYS[i] in MAP)){ cur = i; draw(); return; }
  }
  cur = -1; draw();
}
document.getElementById('file').onchange = e => {
  const f = e.target.files[0]; if(!f) return;
  const r = new FileReader();
  r.onload = () => { img = new Image(); img.onload = fit; img.src = r.result; };
  r.readAsDataURL(f);
};
c.onclick = e => {
  if(cur < 0 || !img) return;
  const r = c.getBoundingClientRect();
  MAP[KEYS[cur]] = [(e.clientX - r.left)/r.width, (e.clientY - r.top)/r.height];
  history.push(KEYS[cur]);
  next();
};
document.getElementById('begin').onclick = () => {
  if(!img){ alert('Load an image first'); return; }
  cur = -1; next();
};
document.getElementById('skip').onclick = () => { if(cur >= 0){ history.push(null); next(); } };
document.getElementById('undo').onclick = () => {
  const k = history.pop();
  if(k){ delete MAP[k]; cur = KEYS.indexOf(k); }
  draw();
};
document.getElementById('save').onclick = async () => {
  const body = JSON.stringify({ keys: MAP, image: document.getElementById('file').files[0]?.name || null });
  const res = await fetch('/save', { method:'POST', headers:{'Content-Type':'application/json'}, body });
  document.getElementById('info').textContent = res.ok ? 'saved keymap_xy.json' : 'save failed';
};
window.onresize = fit;
</script></body></html>
"""


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        if self.path in ("/", "/index.html"):
            existing = {}
            if os.path.exists(OUT):
                try:
                    existing = json.load(open(OUT)).get("keys", {})
                except Exception:
                    existing = {}
            html = PAGE.replace("__KEYS__", json.dumps(KEYS)).replace(
                "__EXISTING__", json.dumps(existing))
            data = html.encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        if self.path != "/save":
            self.send_response(404)
            self.end_headers()
            return
        n = int(self.headers.get("Content-Length", 0))
        payload = json.loads(self.rfile.read(n) or b"{}")
        with open(OUT, "w") as f:
            json.dump(payload, f, indent=1, sort_keys=True)
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")


KEYS = load_keys()
print(f"{len(KEYS)} keys to map: {', '.join(KEYS[:12])}{' ...' if len(KEYS) > 12 else ''}")
print(f"open http://127.0.0.1:{PORT}  (Ctrl-C to stop)")
srv = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
webbrowser.open(f"http://127.0.0.1:{PORT}")
try:
    srv.serve_forever()
except KeyboardInterrupt:
    print("\nstopped")
