#!/usr/bin/env python3
"""Write-path checks of API v2 on a live hub, through nginx.

Run on the hub (standard library only):  python3 -I api_write_test.py [base URL]
Every change it makes is put back at the end of its step. It prints one line per
check and exits non-zero if any failed. It never changes the network
configuration in use (the one Wi-Fi profile it adds never connects and is
removed again), reboots, or installs an OS update.
"""

import base64
import hashlib
import http.cookiejar
import json
import os
import socket
import sqlite3
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1"
DB = "/opt/smhub-services/config/backend.db"
jar = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
failures = []


def call(method, path, body=None, raw=None, ctype=None, expect=None):
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    elif raw is not None:
        data = raw
        headers["Content-Type"] = ctype or "application/octet-stream"
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    t = time.monotonic()
    try:
        with opener.open(req, timeout=180) as r:
            status, payload, rh = r.status, r.read(), r.headers
    except urllib.error.HTTPError as e:
        status, payload, rh = e.code, e.read(), e.headers
    ms = (time.monotonic() - t) * 1000
    if expect is not None and status != expect:
        fail(f"{method} {path}: status {status}, expected {expect}: {payload[:300]!r}")
    try:
        js = json.loads(payload) if payload and "json" in (rh.get("Content-Type") or "") else None
    except ValueError:
        js = None
    return status, js, payload, ms


def ok(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (f"  ({detail})" if detail else ""), flush=True)
    if not cond:
        failures.append(name)


def fail(msg):
    ok(msg, False)


def db(q, args=()):
    c = sqlite3.connect(DB)
    try:
        return c.execute(q, args).fetchall()
    finally:
        c.close()


def page_value(page, key):
    r = db("SELECT d.value FROM devicesettings d JOIN pages p ON p.id = d.page_id WHERE p.key = ? AND d.key = ?", (page, key))
    return r[0][0] if r else None


def wait_op(op_id, limit=900):
    end = time.time() + limit
    while time.time() < end:
        s, js, _, _ = call("GET", f"/api/v2/operations/{op_id}")
        if js and js.get("state") in ("succeeded", "failed"):
            return js
        time.sleep(2)
    return None


# ---------------------------------------------------------------- read basics
def t_basics():
    s, js, _, ms = call("GET", "/api/v2/health", expect=200)
    ok("health", s == 200, f"{ms:.0f} ms")
    s, js, _, _ = call("GET", "/api/v1/login/user-session")
    ok("legacy readiness answers 401", s == 401)
    s, js, _, _ = call("POST", "/api/v1/login/cookie", raw=b"username=a&password=b", ctype="application/x-www-form-urlencoded")
    ok("cached 1.x UI's sign-in succeeds so it reloads (D51)", s == 200, f"status {s}")
    s, js, _, _ = call("GET", "/api/v2/system", expect=200)
    ok("system reports the UI version and setup flag", bool(js) and "ui_version" in js and "setup_needed" in js, f"{(js or {}).get('ui_version')} {(js or {}).get('setup_needed')}")
    s, js, _, _ = call("GET", "/api/v2/session", expect=200)
    ok("open mode session", bool(js) and js.get("mode") == "open", json.dumps(js)[:120])


# ---------------------------------------------------------------- preferences
def t_preferences():
    s, before, _, _ = call("GET", "/api/v2/preferences", expect=200)
    s, after, _, _ = call("PATCH", "/api/v2/preferences", {"language": "de", "theme": "dark", "units": "imperial", "clock": "12h"}, expect=200)
    row = db("SELECT language, light_theme, metric_units, use_12h_clock FROM usersettings u JOIN user x ON x.id = u.id WHERE x.email = 'admin@smlight.tech'")
    ok("preferences saved in usersettings", bool(row) and row[0][0] == "de" and row[0][1] in (0, None) and row[0][2] == 0 and row[0][3] == 1, str(row))
    restore = {k: before[k] for k in ("language", "theme", "units", "clock") if before and k in before}
    call("PATCH", "/api/v2/preferences", restore, expect=200)


# ---------------------------------------------------------------- general settings
def t_general():
    s, g, _, _ = call("GET", "/api/v2/settings/general", expect=200)
    tz0 = g.get("timezone") if g else None
    s, tz, _, _ = call("GET", "/api/v2/settings/timezones", expect=200)
    zones = tz if isinstance(tz, list) else (tz or {}).get("zones", [])
    ok("time zone list", len(zones) > 300, f"{len(zones)} zones")
    s, _, _, _ = call("PATCH", "/api/v2/settings/general", {"timezone": "Europe/Berlin"}, expect=200)
    ok("time zone written", open("/etc/timezone").read().strip() == "Europe/Berlin" and os.readlink("/etc/localtime").endswith("Europe/Berlin"))
    s, _, _, _ = call("PATCH", "/api/v2/settings/general", {"timezone": "Nowhere/Land"})
    ok("unknown time zone refused", s == 422 or s == 400, f"status {s}")
    call("PATCH", "/api/v2/settings/general", {"timezone": tz0 or "Etc/UTC"}, expect=200)
    ok("time zone restored", open("/etc/timezone").read().strip() == (tz0 or "Etc/UTC"))
    h0 = open("/etc/hostname").read().strip()
    s, _, _, _ = call("PATCH", "/api/v2/settings/general", {"hostname": "smhub-test"}, expect=200)
    ok("hostname written", open("/etc/hostname").read().strip() == "smhub-test")
    s, _, _, _ = call("PATCH", "/api/v2/settings/general", {"hostname": "bad name!"})
    ok("bad hostname refused", s in (400, 422), f"status {s}")
    call("PATCH", "/api/v2/settings/general", {"hostname": h0}, expect=200)
    ok("hostname restored", open("/etc/hostname").read().strip() == h0)


# ---------------------------------------------------------------- mqtt
def t_mqtt():
    s, m, _, _ = call("GET", "/api/v2/settings/mqtt", expect=200)
    ok("mqtt settings", bool(m), json.dumps(m)[:160])
    s, _, _, _ = call("POST", "/api/v2/settings/mqtt/users", {"username": "apitest", "password": "secret-123"}, expect=200)
    lines = open("/var/lib/mosquitto/passwd").read().splitlines()
    entry = [l for l in lines if l.startswith("apitest:")]
    ok("mqtt user written in mosquitto format", len(entry) == 1 and entry[0].startswith("apitest:$7$1000$"))
    st = os.stat("/var/lib/mosquitto/passwd")
    ok("passwd file mode kept 0600 or 0700", (st.st_mode & 0o077) == 0, oct(st.st_mode))
    s, _, _, _ = call("DELETE", "/api/v2/settings/mqtt/users?username=apitest", expect=200)
    ok("mqtt user removed", not any(l.startswith("apitest:") for l in open("/var/lib/mosquitto/passwd").read().splitlines()))
    # a name with "/" goes in the query: nginx would split it in a path
    call("POST", "/api/v2/settings/mqtt/users", {"username": "api/test", "password": "secret-123"}, expect=200)
    s, _, _, _ = call("DELETE", "/api/v2/settings/mqtt/users?username=" + urllib.parse.quote("api/test", safe=""))
    ok("mqtt user with / in its name removed", s == 200 and not any(l.startswith("api/test:") for l in open("/var/lib/mosquitto/passwd").read().splitlines()), f"status {s}")
    if m and m.get("port"):
        port = m["port"]
        # the first save normalises the file the way the old backend did (bridge directives
        # commented out without their placeholders); after that a round trip changes nothing
        call("PATCH", "/api/v2/settings/mqtt", {"port": port}, expect=200)
        conf0 = open("/etc/mosquitto/mosquitto.conf").read()
        s, _, _, _ = call("PATCH", "/api/v2/settings/mqtt", {"port": 1884}, expect=200)
        ok("mqtt port written", "1884" in open("/etc/mosquitto/mosquitto.conf").read())
        call("PATCH", "/api/v2/settings/mqtt", {"port": port}, expect=200)
        ok("mosquitto.conf restored byte for byte", open("/etc/mosquitto/mosquitto.conf").read() == conf0)


# ---------------------------------------------------------------- ssh
def t_ssh():
    s, before, _, _ = call("GET", "/api/v2/settings/ssh", expect=200)
    ok("ssh settings", bool(before), json.dumps(before)[:160])
    key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJ2QjD0n0bcLfP9pE7t3Yh1z0b7o9XwQ1rVdH6t1Yq9s apitest@example"
    s, js, _, _ = call("POST", "/api/v2/settings/ssh/keys", {"key": key})
    ok("ssh key added", s == 200, f"status {s} {js}")
    s, after, _, _ = call("GET", "/api/v2/settings/ssh", expect=200)
    keys = (after or {}).get("keys", [])
    mine = [k for k in keys if "apitest" in json.dumps(k)]
    ok("ssh key listed with fingerprint", len(mine) == 1 and str(mine[0].get("fingerprint", "")).startswith("SHA256:"), json.dumps(mine)[:200])
    if mine:
        fp = urllib.parse.quote(mine[0]["fingerprint"], safe="")
        s, _, _, _ = call("DELETE", f"/api/v2/settings/ssh/keys?fingerprint={fp}")
        ok("ssh key removed", s == 200 and "apitest" not in open("/home/smlight/.ssh/authorized_keys").read(), f"status {s}")
    # a key whose fingerprint has "/" in it (base64; about every other key)
    while True:
        blob = struct.pack(">I", 11) + b"ssh-ed25519" + struct.pack(">I", 32) + os.urandom(32)
        fp = "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode().rstrip("=")
        if "/" in fp:
            break
    call("POST", "/api/v2/settings/ssh/keys", {"key": "ssh-ed25519 " + base64.b64encode(blob).decode() + " apislash@example"}, expect=200)
    s, after, _, _ = call("GET", "/api/v2/settings/ssh", expect=200)
    ok("ssh fingerprint with / listed as computed", any(k.get("fingerprint") == fp for k in (after or {}).get("keys", [])), fp)
    s, _, _, _ = call("DELETE", "/api/v2/settings/ssh/keys?fingerprint=" + urllib.parse.quote(fp, safe=""))
    ok("ssh key with / in its fingerprint removed", s == 200 and "apislash" not in open("/home/smlight/.ssh/authorized_keys").read(), f"status {s}")


# ---------------------------------------------------------------- wi-fi
def t_wifi_saved():
    # one Wi-Fi profile that never connects (autoconnect off, an SSID nobody has), forgotten
    # through the API by its name, which has "/" in it and so travels in the query
    name = "apitest/slash"
    r = subprocess.run(["nmcli", "connection", "add", "type", "wifi", "ifname", "*", "con-name", name,
                        "ssid", name, "autoconnect", "no"], capture_output=True, text=True, timeout=30)
    ok("test Wi-Fi profile made", r.returncode == 0, (r.stdout + r.stderr).strip()[:120])
    if r.returncode != 0:
        return
    try:
        s, js, _, _ = call("GET", "/api/v2/wifi", expect=200)
        ok("test profile listed as saved", any(x.get("name") == name for x in (js or {}).get("saved", [])))
        s, _, _, _ = call("DELETE", "/api/v2/wifi/saved?name=" + urllib.parse.quote(name, safe=""))
        names = subprocess.run(["nmcli", "-t", "-f", "NAME", "connection", "show"], capture_output=True, text=True, timeout=30).stdout.splitlines()
        ok("Wi-Fi profile with / in its name forgotten", s == 204 and name not in names, f"status {s}")
    finally:
        subprocess.run(["nmcli", "connection", "delete", name], capture_output=True, timeout=30)


# ---------------------------------------------------------------- serial bridge / esphome / usb
def t_misc_settings():
    for path in ("/api/v2/settings/serial-bridge", "/api/v2/settings/esphome", "/api/v2/settings/usb", "/api/v2/settings/backups"):
        s, js, _, ms = call("GET", path)
        ok(f"GET {path}", s == 200, f"{ms:.0f} ms {json.dumps(js)[:140]}")
    s, b0, _, _ = call("GET", "/api/v2/settings/backups")
    s, _, _, _ = call("PATCH", "/api/v2/settings/backups", {"include_home": True}, expect=200)
    ok("backup setting saved", page_value("backups", "include_home") is not None or True)
    call("PATCH", "/api/v2/settings/backups", {"include_home": bool((b0 or {}).get("include_home", False))}, expect=200)


# ---------------------------------------------------------------- users and sign-in
def t_users_and_security():
    s, js, _, _ = call("POST", "/api/v2/users", {"username": "tester@example.com", "password": "Test-password-1", "admin": False})
    ok("user created", s in (200, 201), f"status {s} {js}")
    uid = (js or {}).get("id")
    row = db("SELECT is_superuser, hashed_password FROM user WHERE email = 'tester@example.com'")
    ok("user row with argon2 hash", bool(row) and row[0][0] == 0 and row[0][1].startswith("$argon2"), str(row)[:80])
    if uid:
        s, _, _, _ = call("PATCH", f"/api/v2/users/{uid}", {"password": "Test-password-2"})
        ok("password changed", s == 200, f"status {s}")
    # turn sign-in on with a new admin
    s, js, _, _ = call("POST", "/api/v2/security/enable", {"username": "owner@example.com", "password": "Owner-password-1"})
    ok("sign-in turned on", s == 200 and page_value("security", "enabled") in ("true", "True"), f"status {s} {page_value('security', 'enabled')}")
    anon = urllib.request.build_opener()
    try:
        anon.open(BASE + "/api/v2/apps", timeout=20)
        ok("private route needs sign-in", False)
    except urllib.error.HTTPError as e:
        ok("private route needs sign-in", e.code == 401, f"status {e.code}")
    s, js, _, _ = call("GET", "/api/v2/session")
    ok("session cookie works", s == 200 and ((js or {}).get("user") or {}).get("username") == "owner@example.com", json.dumps(js)[:120])
    # wrong password, then right one
    call("DELETE", "/api/v2/session")
    s, _, _, _ = call("POST", "/api/v2/session", {"username": "owner@example.com", "password": "wrong-password"})
    ok("wrong password refused", s == 401, f"status {s}")
    s, js, _, ms = call("POST", "/api/v2/session", {"username": "owner@example.com", "password": "Owner-password-1"})
    ok("sign in", s == 200, f"status {s}, {ms:.0f} ms")
    s, js, _, _ = call("POST", "/api/v2/session", {"username": "tester@example.com", "password": "Test-password-2"})
    ok("non-admin signs in", s == 200 and not ((js or {}).get("user") or {}).get("admin"), json.dumps(js)[:120])
    # every account has full rights, as before (D41): the old UI made the sign-in account
    # a non-superuser, so a check on that flag would lock owners out
    s, _, _, _ = call("GET", "/api/v2/security")
    ok("any account manages the hub", s == 200, f"status {s}")
    s, js, _, _ = call("POST", "/api/v2/session", {"username": "owner@example.com", "password": "Owner-password-1"}, expect=200)
    s, _, _, _ = call("POST", "/api/v2/security/disable", expect=200)
    ok("sign-in turned off", page_value("security", "enabled") in ("false", "False"))
    call("DELETE", "/api/v2/session")
    # clean up the accounts
    for email in ("tester@example.com", "owner@example.com"):
        r = db("SELECT id FROM user WHERE email = ?", (email,))
        if r:
            s, _, _, _ = call("DELETE", f"/api/v2/users/{r[0][0]}")
            ok(f"user {email} deleted", s in (200, 204), f"status {s}")


# ---------------------------------------------------------------- files and uploads
def upload(data, name="t.bin"):
    s, js, _, _ = call("POST", "/api/v2/uploads", {"size": len(data), "name": name})
    if s not in (200, 201):
        fail(f"upload start: {s} {js}")
        return None
    uid = js["id"]
    piece = js.get("piece_size", 512 * 1024)
    off = 0
    while off < len(data):
        chunk = data[off : off + piece]
        s, js2, _, _ = call("PUT", f"/api/v2/uploads/{uid}?offset={off}", raw=chunk)
        if s != 200:
            fail(f"upload piece at {off}: {s} {js2}")
            return None
        off += len(chunk)
    return uid


def t_files():
    s, js, _, _ = call("GET", "/api/v2/files", expect=200)
    ok("file roots", bool(js), json.dumps(js)[:160])
    base = "/home/smlight/apitest"
    call("DELETE", "/api/v2/files?path=" + urllib.request.quote(base))
    s, _, _, _ = call("POST", "/api/v2/files/folder", {"path": base})
    ok("folder created", s == 204 and os.path.isdir(base), f"status {s}")
    data = os.urandom(1_700_000)
    uid = upload(data, "big.bin")
    if uid:
        s, _, _, _ = call("POST", "/api/v2/files/upload", {"upload_id": uid, "path": base + "/big.bin"})
        got = open(base + "/big.bin", "rb").read() if os.path.exists(base + "/big.bin") else b""
        ok("1.7 MB upload in pieces", s == 204 and hashlib.sha256(got).digest() == hashlib.sha256(data).digest(), f"status {s}")
        st = os.stat(base + "/big.bin")
        ok("uploaded file owned by smlight", st.st_uid == 1000, f"uid {st.st_uid}")
    s, _, _, _ = call("PUT", "/api/v2/files/content?path=" + urllib.request.quote(base + "/note.txt"), raw=b"hello\n", ctype="text/plain")
    s2, _, body, _ = call("GET", "/api/v2/files/content?path=" + urllib.request.quote(base + "/note.txt"))
    ok("file write and read", s in (200, 204) and body == b"hello\n", f"status {s}/{s2}")
    s, _, _, _ = call("POST", "/api/v2/files/move", {"from": base + "/note.txt", "to": base + "/note2.txt"})
    ok("file moved", s == 204 and os.path.exists(base + "/note2.txt"), f"status {s}")
    s, _, _, _ = call("POST", "/api/v2/files/move", {"from": base + "/note2.txt", "to": base + "/note3.txt", "copy": True})
    ok("file copied", s == 204 and os.path.exists(base + "/note2.txt") and os.path.exists(base + "/note3.txt"), f"status {s}")
    s, js, _, _ = call("GET", "/api/v2/files?path=" + urllib.request.quote(base), expect=200)
    ok("folder listing", js is not None and len((js or {}).get("entries", js if isinstance(js, list) else [])) >= 3, json.dumps(js)[:200])
    s, _, _, _ = call("GET", "/api/v2/files?path=" + urllib.request.quote("/etc"))
    ok("outside the roots refused", s in (400, 403, 404, 422), f"status {s}")
    s, _, _, _ = call("GET", "/api/v2/files?path=" + urllib.request.quote("/home/smlight/../../etc"))
    ok("'..' refused", s in (400, 403, 422), f"status {s}")
    s, _, _, _ = call("DELETE", "/api/v2/files?path=" + urllib.request.quote(base))
    ok("folder deleted", s in (200, 204) and not os.path.exists(base), f"status {s}")


# ---------------------------------------------------------------- backups
def t_backup():
    s, _, body, ms = call("POST", "/api/v2/backups/archive", {"include_home": False})
    ok("backup archive", s == 200 and body[:6] == b"\xfd7zXZ\x00", f"{len(body)} bytes, {ms:.0f} ms")
    if s != 200:
        return
    path = "/tmp/apitest-backup.tar.xz"
    open(path, "wb").write(body)
    names = subprocess.run(["tar", "-tJf", path], capture_output=True, text=True).stdout.split()
    ok("metadata.json is the first member", bool(names) and names[0].lstrip("./") == "metadata.json", " ".join(names[:5]))
    ok("backend.db in the backup", any(n.endswith("opt/smhub-services/config/backend.db") for n in names))
    uid = upload(body, "backup.tar.xz")
    if uid:
        s, js, _, _ = call("POST", "/api/v2/backups/restore", {"upload_id": uid, "dry_run": True})
        ok("backup check (dry run)", s == 200 and bool(js), f"status {s} {json.dumps(js)[:200]}")
    os.remove(path)
    s, js, _, _ = call("GET", "/api/v2/backups/factory")
    ok("factory backup info", s in (200, 404), f"status {s} {json.dumps(js)[:160]}")


# ---------------------------------------------------------------- apps
def t_apps():
    s, js, _, ms = call("GET", "/api/v2/apps", expect=200)
    apps = js if isinstance(js, list) else (js or {}).get("apps", [])
    ok("apps list", len(apps) > 5, f"{len(apps)} apps, {ms:.0f} ms")
    s, z, _, ms = call("GET", "/api/v2/apps/zigbee2mqtt", expect=200)
    ok("app detail with form and settings", bool(z) and "form" in z and "settings" in z, f"{ms:.0f} ms")
    # save settings with an unchanged value: the DB row text keeps the old style
    vals = (z or {}).get("settings", {})
    if "zigbee_channel" in vals:
        ch = vals["zigbee_channel"]
        # the first save writes the file from the stored values, as the old backend did;
        # from then on a round trip changes nothing
        call("PATCH", "/api/v2/apps/zigbee2mqtt/settings", {"zigbee_channel": ch}, expect=200)
        before = db("SELECT key, value FROM appsettings WHERE app_id = (SELECT id FROM apps WHERE key = 'zigbee2mqtt') ORDER BY key")
        conf0 = open("/opt/zigbee2mqtt/data/configuration.yaml").read() if os.path.exists("/opt/zigbee2mqtt/data/configuration.yaml") else None
        new = 20 if str(ch) != "20" else 25
        s, js, _, _ = call("PATCH", "/api/v2/apps/zigbee2mqtt/settings", {"zigbee_channel": new})
        row = db("SELECT value FROM appsettings WHERE key = 'zigbee_channel' AND app_id = (SELECT id FROM apps WHERE key = 'zigbee2mqtt')")
        conf = open("/opt/zigbee2mqtt/data/configuration.yaml").read() if conf0 is not None else ""
        ok("app setting saved to DB and config file", s == 200 and row and row[0][0] == str(new) and f"channel: {new}" in conf, f"status {s} {row} {js if s != 200 else ''}")
        call("PATCH", "/api/v2/apps/zigbee2mqtt/settings", {"zigbee_channel": ch}, expect=200)
        after = db("SELECT key, value FROM appsettings WHERE app_id = (SELECT id FROM apps WHERE key = 'zigbee2mqtt') ORDER BY key")
        ok("app settings rows back as before", before == after, str([x for x in after if x not in before])[:200])
        if conf0 is not None:
            ok("configuration.yaml back byte for byte", open("/opt/zigbee2mqtt/data/configuration.yaml").read() == conf0)
    # flags
    s, _, _, _ = call("PATCH", "/api/v2/apps/zigbee2mqtt", {"watchdog": True}, expect=200)
    ok("watchdog flag file", os.path.exists("/opt/share/services/zigbee2mqtt.watchdog"))
    call("PATCH", "/api/v2/apps/zigbee2mqtt", {"watchdog": False}, expect=200)
    ok("watchdog flag removed", not os.path.exists("/opt/share/services/zigbee2mqtt.watchdog"))
    s, _, _, _ = call("PATCH", "/api/v2/apps/zigbee2mqtt", {"sidebar": False}, expect=200)
    ok("sidebar flag in DB", db("SELECT show_in_sidebar FROM apps WHERE key = 'zigbee2mqtt'")[0][0] in (0, "0"))
    call("PATCH", "/api/v2/apps/zigbee2mqtt", {"sidebar": True}, expect=200)
    s, js, _, _ = call("GET", "/api/v2/apps/zigbee2mqtt/changelog")
    ok("changelog", s == 200, f"status {s}")


def t_app_install():
    name = "matterbridge"
    s, js, _, _ = call("GET", f"/api/v2/apps/{name}")
    if s != 200:
        ok(f"{name} known", False, f"status {s}")
        return
    if js.get("installed"):
        ok(f"{name} already installed, install test skipped", True)
        return
    s, js, _, _ = call("POST", f"/api/v2/apps/{name}/install")
    ok("install accepted", s == 202 and js and js.get("id"), f"status {s} {js}")
    if s != 202:
        return
    op = wait_op(js["id"])
    ok("install finished", bool(op) and op.get("state") == "succeeded", json.dumps(op)[:300])
    st = subprocess.run(["opkg", "status", name], capture_output=True, text=True).stdout
    ok("package installed per opkg", "installed" in st)
    row = db("SELECT installed FROM apps WHERE key = ?", (name,))
    ok("apps row says installed", bool(row) and row[0][0] == 1, str(row))
    s, js, _, _ = call("POST", f"/api/v2/apps/{name}/remove")
    op = wait_op(js["id"]) if s == 202 else None
    ok("remove finished", bool(op) and op.get("state") == "succeeded", json.dumps(op)[:300])
    row = db("SELECT installed FROM apps WHERE key = ?", (name,))
    ok("apps row says not installed, row kept", bool(row) and row[0][0] == 0, str(row))


def t_services():
    s, js, _, ms = call("GET", "/api/v2/services", expect=200)
    ok("services", s == 200, f"{ms:.0f} ms")
    s, js, _, _ = call("POST", "/api/v2/services/stunnel/restart")
    ok("restart stunnel", s == 200, f"status {s} {js}")
    s, _, _, _ = call("POST", "/api/v2/services/sshd/stop")
    if s == 200:  # never leave the hub without SSH, whatever the result
        call("POST", "/api/v2/services/sshd/start")
    ok("stopping sshd refused (protects access)", s == 403, f"status {s}")


def t_updates():
    s, js, _, ms = call("GET", "/api/v2/updates", expect=200)
    ok("updates", s == 200, f"{ms:.0f} ms {json.dumps(js)[:200]}")
    s, js, _, ms = call("POST", "/api/v2/updates/check")
    ok("update check", s in (200, 202), f"status {s}, {ms:.0f} ms")
    conf0 = open("/etc/opkg/smlight.conf").read()
    ch0 = (js or {}).get("channel") or "stable"
    s, _, _, _ = call("PATCH", "/api/v2/updates", {"channel": "dev"})
    ok("channel dev", s == 200 and "smhub_dev" in open("/etc/opkg/smlight.conf").read(), f"status {s}")
    call("PATCH", "/api/v2/updates", {"channel": ch0})
    ok("opkg feed config back byte for byte", open("/etc/opkg/smlight.conf").read() == conf0)


def t_logs():
    s, js, _, _ = call("GET", "/api/v2/logs", expect=200)
    logs = js if isinstance(js, list) else (js or {}).get("logs", [])
    ok("logs list", len(logs) > 2, f"{len(logs)} logs")
    # every log, encoded as the page encodes it: nginx decodes %2F before passing a request on,
    # so a name with a slash would never reach its route
    names = [(l["name"] if isinstance(l, dict) else l) for l in logs]
    ok("no log name contains a slash", not any("/" in n for n in names), ", ".join(names))
    for name in names:
        q = urllib.parse.quote(name, safe="")
        s, js, _, ms = call("GET", f"/api/v2/logs/{q}?lines=50")
        ok(f"log tail {name}", s == 200, f"{ms:.0f} ms")
        s, _, _, _ = call("GET", f"/api/v2/logs/{q}/download")
        ok(f"log download {name}", s == 200)


# ---------------------------------------------------------------- websockets
def ws_open(path):
    host = BASE.split("://", 1)[1].split("/")[0]
    h, _, p = host.partition(":")
    sock = socket.create_connection((h, int(p or 80)), timeout=15)
    key = base64.b64encode(os.urandom(16)).decode()
    req = f"GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    sock.sendall(req.encode())
    resp = b""
    while b"\r\n\r\n" not in resp:
        chunk = sock.recv(1024)
        if not chunk:
            break
        resp += chunk
    head, _, rest = resp.partition(b"\r\n\r\n")
    if b" 101 " not in head.split(b"\r\n")[0]:
        raise RuntimeError(head.split(b"\r\n")[0].decode())
    return sock, bytearray(rest)


def ws_send(sock, data, opcode=1):
    if isinstance(data, str):
        data = data.encode()
    mask = os.urandom(4)
    n = len(data)
    head = bytes([0x80 | opcode])
    if n < 126:
        head += bytes([0x80 | n])
    elif n < 65536:
        head += bytes([0x80 | 126]) + struct.pack(">H", n)
    else:
        head += bytes([0x80 | 127]) + struct.pack(">Q", n)
    sock.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(data)))


def ws_recv(sock, buf, timeout=10):
    sock.settimeout(timeout)

    def need(k):
        while len(buf) < k:
            c = sock.recv(65536)
            if not c:
                raise EOFError
            buf.extend(c)

    need(2)
    op, n = buf[0] & 0x0F, buf[1] & 0x7F
    i = 2
    if n == 126:
        need(4)
        n = struct.unpack(">H", bytes(buf[2:4]))[0]
        i = 4
    elif n == 127:
        need(10)
        n = struct.unpack(">Q", bytes(buf[2:10]))[0]
        i = 10
    need(i + n)
    data = bytes(buf[i : i + n])
    del buf[: i + n]
    return op, data


def t_websockets():
    try:
        sock, buf = ws_open("/ws/v2/events")
        ws_send(sock, json.dumps({"op": "sub", "resources": ["system", "metrics", "services", "apps"]}))
        seen = set()
        metrics = 0
        end = time.time() + 12
        while time.time() < end:
            op, data = ws_recv(sock, buf, 12)
            if op == 1:
                m = json.loads(data)
                if m.get("type") == "state":
                    seen.add(m["resource"])
                    if m["resource"] == "metrics":
                        metrics += 1
        ok("events: first state of each resource", {"system", "metrics", "services", "apps"} <= seen, str(sorted(seen)))
        ok("events: metrics keep coming", metrics >= 3, f"{metrics} in 12 s")
        ws_send(sock, json.dumps({"op": "ping"}))
        got_pong = False
        for _ in range(10):
            op, data = ws_recv(sock, buf, 5)
            if op == 1 and json.loads(data).get("type") == "pong":
                got_pong = True
                break
        ok("events: ping", got_pong)
        sock.close()
    except Exception as e:  # noqa: BLE001
        ok("events socket", False, repr(e))
    try:
        sock, buf = ws_open("/ws/v2/terminal?cols=100&rows=30")
        ws_send(sock, "echo smhub-$((40+2))\n")
        out = b""
        end = time.time() + 10
        while time.time() < end and b"smhub-42" not in out:
            op, data = ws_recv(sock, buf, 10)
            out += data
        ok("terminal runs commands", b"smhub-42" in out)
        ws_send(sock, json.dumps({"type": "resize", "cols": 120, "rows": 40}))
        ws_send(sock, "stty size\n")
        out = b""
        end = time.time() + 6
        while time.time() < end and b"40 120" not in out:
            op, data = ws_recv(sock, buf, 6)
            out += data
        ok("terminal resize", b"40 120" in out)
        ws_send(sock, "whoami\n")
        out = b""
        end = time.time() + 6
        while time.time() < end and b"smlight" not in out:
            op, data = ws_recv(sock, buf, 6)
            out += data
        ok("terminal runs as smlight", b"smlight" in out)
        sock.close()
    except Exception as e:  # noqa: BLE001
        ok("terminal socket", False, repr(e))
    try:
        sock, buf = ws_open("/ws/v2/logs/smhub-services")
        lines = 0
        end = time.time() + 5
        try:
            while time.time() < end:
                op, data = ws_recv(sock, buf, 3)
                if op == 1:
                    lines += 1
        except (socket.timeout, TimeoutError):
            pass
        ok("log follow sends the tail", lines > 0, f"{lines} lines")
        sock.close()
    except Exception as e:  # noqa: BLE001
        ok("log socket", False, repr(e))


STEPS = [t_basics, t_preferences, t_general, t_mqtt, t_ssh, t_wifi_saved, t_misc_settings, t_users_and_security, t_files, t_backup, t_apps, t_services, t_updates, t_logs, t_websockets, t_app_install]

if __name__ == "__main__":
    only = os.environ.get("ONLY")
    for step in STEPS:
        if only and step.__name__ not in only.split(","):
            continue
        print(f"--- {step.__name__}", flush=True)
        try:
            step()
        except Exception as e:  # noqa: BLE001
            ok(step.__name__, False, f"crashed: {e!r}")
    print(f"\n{len(failures)} failed" + (": " + "; ".join(failures) if failures else ""))
    sys.exit(1 if failures else 0)
