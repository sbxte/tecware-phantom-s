#!/usr/bin/env python3
"""Restore a saved keyboard state.  python3 restore.py keyboard_state.json"""
import json
import sys
import phantom

state = json.load(open(sys.argv[1] if len(sys.argv) > 1 else "keyboard_state.json"))
d = phantom.Phantom(phantom.find_device())
if "area" in state:
    d.write_area(bytes.fromhex(state["area"]))
if "matrix" in state:
    d._write_blob(0x1A, bytes.fromhex(state["matrix"]))
d.close()
print("restored", {k: len(v) // 2 for k, v in state.items() if k in ("area", "matrix")})
