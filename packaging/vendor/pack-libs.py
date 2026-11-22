# SPDX-License-Identifier: GPL-3.0-only
# Copyright 2026 SMLIGHT. Licensed under GPL-3.0 (LICENSES/GPL-3.0.txt), as the libraries it packs.
import importlib.metadata as md, re, sys, os, tarfile
roots = ["universal-silabs-flasher", "smlight-cc-flasher", "gpiod"]
seen = {}
def norm(n): return re.sub(r"[-_.]+", "-", n).lower()
def walk(name):
    n = norm(name)
    if n in seen: return
    try: d = md.distribution(name)
    except md.PackageNotFoundError: seen[n] = None; return
    seen[n] = d
    for r in d.requires or []:
        if "extra ==" in r: continue
        m = re.match(r"^\s*([A-Za-z0-9_.\-]+)", r)
        if m: walk(m.group(1))
for r in roots: walk(r)
site = [p for p in sys.path if p.endswith("site-packages")][0]
tops = set()
for d in seen.values():
    if d is None: continue
    tops.add(os.path.basename(str(d._path)))
    tl = d.read_text("top_level.txt")
    names = [t.strip() for t in tl.splitlines() if t.strip()] if tl else []
    if not names:
        names = {str(f).split("/")[0] for f in (d.files or []) if not str(f).startswith("..") and ".dist-info" not in str(f).split("/")[0]}
    for t in names:
        for cand in (t, t + ".py"):
            if os.path.exists(os.path.join(site, cand)): tops.add(cand)
def skip(i):
    if "__pycache__" in i.name or i.name.endswith("/RECORD") or i.name.endswith("/INSTALLER") or i.name.endswith("/REQUESTED"):
        return None
    i.uid = i.gid = 0
    i.uname = i.gname = "root"
    return i
with tarfile.open("/tmp/flasher-lib.tgz", "w:gz") as tf:
    for t in sorted(tops):
        tf.add(os.path.join(site, t), arcname=t, filter=skip)
print(len(tops), "top-level entries")
print(" ".join(sorted(x for x in tops if "dist-info" in x)))
print(os.path.getsize("/tmp/flasher-lib.tgz") // 1024, "KB")
