#!/usr/bin/env python3
"""The hub's own hardware through API v2 (docs/peripherals.md), on a live hub.

Run on the hub (standard library only):  python3 -I hardware_test.py [base URL] [--sound]
Every change is put back at the end of its step: settings, the light ring's look, the
LEDs, infrared codes, tokens. Without --sound it makes no sound; with it, it plays one
short beep and stops one sound. It never learns infrared (that needs a remote).
"""

import json
import sys
import time
import urllib.error
import urllib.request

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
BASE = ARGS[0] if ARGS else "http://127.0.0.1"
SOUND = "--sound" in sys.argv
failures = []


def call(method, path, body=None, token=None, expect=None):
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Content-Type": "application/json"} if body is not None else {}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            status, payload, ctype = r.status, r.read(), r.headers.get("Content-Type") or ""
    except urllib.error.HTTPError as e:
        status, payload, ctype = e.code, e.read(), e.headers.get("Content-Type") or ""
    js = json.loads(payload) if payload and "json" in ctype else None
    if expect is not None and status != expect:
        ok(f"{method} {path} answers {expect}", False, f"status {status}: {payload[:200]!r}")
    return status, js


def ok(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (f"  ({detail})" if detail else ""), flush=True)
    if not cond:
        failures.append(name)


def t_read():
    s, hw = call("GET", "/api/v2/hardware", expect=200)
    ok("hardware resource", bool(hw) and {"buzzer", "light", "leds", "button", "button2", "esphome"} <= set(hw), json.dumps(hw)[:200] if hw else "")
    if hw:
        print("     owners:", {k: hw[k]["owner"] for k in ("buzzer", "light", "leds", "button", "button2")}, "esphome:", hw["esphome"])
        # the buttons are the kernel's on every model: read by the hub itself, never by ESPHome
        ok("buttons read from the kernel", all(hw[k]["owner"] in ("linux", "none") for k in ("button", "button2")) and hw["button2"]["available"], json.dumps({k: hw[k] for k in ("button", "button2")}))
    s, st = call("GET", "/api/v2/settings/hardware", expect=200)
    ok("hardware settings", bool(st) and "sounds" in st and "leds" in st)
    s, ir = call("GET", "/api/v2/ir", expect=200)
    ok("infrared resource", bool(ir) and "codes" in ir)
    return hw, st, ir


def t_settings(before):
    s, after = call("PATCH", "/api/v2/settings/hardware", {"sounds": {"mute": True, "quiet": {"enabled": True, "from": "21:30"}}}, expect=200)
    ok("settings merge", bool(after) and after["sounds"]["mute"] and after["sounds"]["quiet"]["from"] == "21:30" and after["sounds"]["quiet"]["to"] == before["sounds"]["quiet"]["to"])
    s, js = call("PATCH", "/api/v2/settings/hardware", {"sounds": {"quiet": {"from": "25:00"}}})
    ok("a bad time is refused with its field", s == 422 and js and js["error"]["field"] == "sounds.quiet", f"{s} {js}")
    s, js = call("PATCH", "/api/v2/settings/hardware", {"button": {"long": {"action": "call_url", "url": "ftp://x"}}})
    ok("a bad button address is refused", s == 422, f"{s}")
    call("PATCH", "/api/v2/settings/hardware", before, expect=200)
    s, back = call("GET", "/api/v2/settings/hardware", expect=200)
    ok("settings put back", back == before)


def t_tokens():
    s, tk = call("POST", "/api/v2/tokens", {"name": "hardware test"}, expect=201)
    ok("token made, value shown once", bool(tk) and tk.get("token", "").startswith("smhub_"))
    if not tk:
        return
    s, js = call("GET", "/api/v2/hardware", token=tk["token"])
    ok("token reads the hardware", s == 200, f"{s}")
    s, js = call("GET", "/api/v2/tokens", token=tk["token"])
    ok("token cannot manage tokens", s == 403, f"{s}")
    s, js = call("GET", "/api/v2/hardware", token="smhub_wrong")
    ok("a wrong token is refused", s == 401, f"{s}")
    s, js = call("GET", "/api/v2/tokens", expect=200)
    ok("token listed without its value", bool(js) and any(t["id"] == tk["id"] and "token" not in t for t in js))
    call("DELETE", f"/api/v2/tokens/{tk['id']}", expect=204)
    s, js = call("GET", "/api/v2/hardware", token=tk["token"])
    ok("a revoked token is refused", s == 401, f"{s}")


def wait_light(pred, limit=6):
    end = time.time() + limit
    hw = None
    while time.time() < end:
        s, hw = call("GET", "/api/v2/hardware")
        if hw and pred(hw["light"]):
            return hw
        time.sleep(0.5)
    return hw


def t_light(hw):
    l = hw["light"]
    if not l["available"]:
        print("     (no light ring here)")
        return
    if l["owner"] == "esphome" and not l["known"]:
        ok("light state known from ESPHome", False, "ESPHome did not report the light")
        return
    before = {"on": l["on"], "color": l["color"], "brightness": l["brightness"], "effect": l["effect"]}
    s, js = call("PATCH", "/api/v2/hardware/light", {"color": "#00ff00", "brightness": 30, "effect": "None"}, expect=200)
    got = wait_light(lambda x: x["on"] and x["color"] in ("#00ff00",) and abs(x["brightness"] - 30) <= 2)
    ok("light changed and reported back", bool(got) and got["light"]["color"] == "#00ff00", json.dumps(got["light"]) if got else "")
    # the first effect, and Rainbow, which ESPHome's firmware calls "Rainbow Effect": the hub's
    # name goes out and comes back the same
    for e in dict.fromkeys(l["effects"][:1] + [x for x in l["effects"] if x == "Rainbow"]):
        call("PATCH", "/api/v2/hardware/light", {"effect": e}, expect=200)
        got = wait_light(lambda x: x["effect"] == e)
        ok(f"effect {e} shows", bool(got) and got["light"]["effect"] == e, json.dumps(got["light"]) if got else "")
    s, js = call("PATCH", "/api/v2/hardware/light", {"effect": "Not an effect"})
    ok("an unknown effect is refused", s == 422, f"{s}")
    # brightness alone, while the ring is off, turns it on and sticks
    call("PATCH", "/api/v2/hardware/light", {"on": False}, expect=200)
    wait_light(lambda x: not x["on"])
    call("PATCH", "/api/v2/hardware/light", {"brightness": 60}, expect=200)
    got = wait_light(lambda x: x["on"] and abs(x["brightness"] - 60) <= 2)
    ok("brightness while off turns the ring on", bool(got) and got["light"]["on"] and abs(got["light"]["brightness"] - 60) <= 2, json.dumps(got["light"]) if got else "")
    if not l["per_led"]:
        s, js = call("PATCH", "/api/v2/hardware/light", {"pixels": ["#ff0000"] * 12})
        ok("single LEDs refused on ESPHome", s == 501 or s == 422 or s == 400, f"{s}")
    # the look first (it lights the ring), then off if it was off: ESPHome keeps the look it had
    look = {k: before[k] for k in ("color", "brightness", "effect")}
    call("PATCH", "/api/v2/hardware/light", look, expect=200)
    wait_light(lambda x: x["on"] and x["color"] == before["color"] and abs(x["brightness"] - before["brightness"]) <= 2)
    if not before["on"]:
        call("PATCH", "/api/v2/hardware/light", {"on": False}, expect=200)
    got = wait_light(lambda x: x["on"] == before["on"] and abs(x["brightness"] - before["brightness"]) <= 2)
    ok("light put back, brightness too", bool(got) and got["light"]["on"] == before["on"] and abs(got["light"]["brightness"] - before["brightness"]) <= 2, json.dumps(got["light"]) if got else "")


def t_leds(hw):
    l = hw["leds"]
    if not l["available"] or (l["owner"] == "esphome" and not l["known"]):
        ok("LEDs known", False, json.dumps(l))
        return
    before = {"power": l["power"], "blue": l["blue"]}
    call("PATCH", "/api/v2/hardware/leds", {"blue": not before["blue"]}, expect=200)
    end = time.time() + 5
    seen = None
    while time.time() < end:
        s, cur = call("GET", "/api/v2/hardware")
        seen = cur["leds"]["blue"] if cur else None
        if seen == (not before["blue"]):
            break
        time.sleep(0.5)
    ok("blue LED switched and reported back", seen == (not before["blue"]), f"{seen}")
    call("PATCH", "/api/v2/hardware/leds", before, expect=200)
    time.sleep(1)
    s, cur = call("GET", "/api/v2/hardware")
    ok("LEDs put back", cur and cur["leds"]["blue"] == before["blue"] and cur["leds"]["power"] == before["power"])


def t_ir(ir):
    if not ir["available"]:
        print("     (no infrared here)")
        return
    s, code = call("POST", "/api/v2/ir/codes", {"name": "Test power", "device": "Test TV", "protocol": "nec", "scancode": "0x408"}, expect=201)
    ok("a code saved by protocol", bool(code) and code["scancode"] == "0x408")
    if not code:
        return
    s, js = call("POST", f"/api/v2/ir/codes/{code['id']}/send", expect=200)
    ok("the code is sent", s == 200, f"{s} {js}")
    s, js = call("POST", "/api/v2/ir/send", {"raw": [9000, 4500, 560, 560, 560], "carrier": 38000}, expect=200)
    ok("raw timings are sent", s == 200)
    s, js = call("POST", "/api/v2/ir/send", {"raw": [9000, 4500]})
    ok("an even number of timings is refused", s == 422, f"{s}")
    s, js = call("PATCH", f"/api/v2/ir/codes/{code['id']}", {"favourite": True, "name": "Renamed"}, expect=200)
    ok("a code renamed and starred", bool(js) and js["favourite"] and js["name"] == "Renamed")
    s, exp = call("GET", "/api/v2/ir/export", expect=200)
    ok("export holds the code", bool(exp) and any(c["id"] == code["id"] for c in exp["codes"]))
    call("DELETE", f"/api/v2/ir/codes/{code['id']}", expect=204)
    s, ir2 = call("GET", "/api/v2/ir", expect=200)
    ok("the code is gone", bool(ir2) and not any(c["id"] == code["id"] for c in ir2["codes"]))


def t_sound(hw):
    if not hw["buzzer"]["available"]:
        print("     (no buzzer here)")
        return
    s, js = call("POST", "/api/v2/hardware/buzzer/play", {"rtttl": "x:d=4,o=5,b=100:q"})
    ok("a bad melody is refused", s == 422, f"{s}")
    if not SOUND:
        print("     (sounds skipped; pass --sound)")
        return
    s, js = call("POST", "/api/v2/hardware/buzzer/play", {"sound": "beep"}, expect=200)
    ok("beep plays", bool(js) and js["result"] == "playing", json.dumps(js))
    s, js = call("POST", "/api/v2/hardware/buzzer/play", {"sound": "beep"}, expect=200)
    ok("the same sound at once is dropped", bool(js) and js["result"] == "dropped" and js["reason"] == "repeat", json.dumps(js))
    time.sleep(1.2)
    call("POST", "/api/v2/hardware/buzzer/stop", {"clear": True}, expect=200)


if __name__ == "__main__":
    hw, st, ir = t_read()
    if hw and st and ir:
        for step in (lambda: t_settings(st), t_tokens, lambda: t_light(hw), lambda: t_leds(hw), lambda: t_ir(ir), lambda: t_sound(hw)):
            try:
                step()
            except Exception as e:  # noqa: BLE001 - report and go on
                ok(f"step raised {e!r}", False)
    print(f"\n{len(failures)} failed" if failures else "\nall passed")
    sys.exit(1 if failures else 0)
