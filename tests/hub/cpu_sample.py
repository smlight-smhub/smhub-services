#!/usr/bin/env python3
"""Prints, once a second, the hub's total CPU use, the busiest processes and how many
processes were started (short-lived commands are otherwise invisible).

    python3 -I cpu_sample.py <seconds>

The sampler's own CPU is shown separately ("sampler"), so it can be subtracted.
"""
import os
import sys
import time

HZ = os.sysconf("SC_CLK_TCK")
ME = str(os.getpid())


def stat():
    tot = idle = forks = 0
    with open("/proc/stat") as f:
        for line in f:
            if line.startswith("cpu "):
                v = [int(x) for x in line.split()[1:]]
                tot, idle = sum(v), v[3] + v[4]
            elif line.startswith("processes "):
                forks = int(line.split()[1])
    return tot, idle, forks


def procs():
    out = {}
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            with open(f"/proc/{p}/stat") as f:
                s = f.read()
            name = s[s.index("(") + 1 : s.rindex(")")]
            f2 = s[s.rindex(")") + 2 :].split()
            out[p] = (name, int(f2[11]) + int(f2[12]))
        except (OSError, ValueError):
            pass
    return out


secs = int(sys.argv[1]) if len(sys.argv) > 1 else 30
t0, i0, k0 = stat()
p0 = procs()
busy_sum = 0.0
for n in range(secs):
    time.sleep(1)
    t1, i1, k1 = stat()
    p1 = procs()
    busy = 100 * (1 - (i1 - i0) / max(1, t1 - t0))
    busy_sum += busy
    used = []
    new = []
    for p, (name, ticks) in p1.items():
        if p in p0:
            d = ticks - p0[p][1]
        else:
            d = ticks  # started during this second
            new.append(name)
        if d > 0:
            used.append((d, "sampler" if p == ME else name))
    used.sort(reverse=True)
    tops = ", ".join(f"{name} {d * 100 // HZ}%" for d, name in used[:4])
    started = k1 - k0
    seen = ",".join(sorted(set(new)))
    print(f"{n + 1:3d}s  cpu {busy:5.1f}%  {tops}  | started {started}{' (' + seen + ')' if seen else ''}", flush=True)
    t0, i0, k0, p0 = t1, i1, k1, p1
print(f"average {busy_sum / secs:.1f}%", flush=True)
