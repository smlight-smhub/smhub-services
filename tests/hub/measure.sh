#!/bin/sh
# Measure smhubd against the targets in docs/vision.md, on a live hub, as root.
#   measure.sh [seconds of idle sampling, default 600]
# Run it with no browser open on the hub for the idle numbers.
set -u
IDLE=${1:-600}
pid=""
for d in /proc/[0-9]*; do
    [ "$(readlink "$d/exe" 2>/dev/null)" = /opt/smhub-services/bin/smhubd ] && pid=${d#/proc/}
done
[ -n "$pid" ] || { echo "smhubd is not running"; exit 1; }
hz=100

mem() { # private memory where the kernel reports it; the hub's kernel has no smaps, so RSS split
    f="/proc/$pid/smaps_rollup"
    [ -r "$f" ] || f="/proc/$pid/smaps"
    if [ -r "$f" ]; then
        awk '/^Private_(Clean|Dirty)/ {s += $2} END {printf "%.1f MiB", s / 1024}' "$f"
    else
        awk '/^RssAnon/ {a = $2} /^RssFile/ {f = $2} END {printf "RSS %.1f MiB = %.1f MiB anonymous + %.1f MiB of the binary", (a + f) / 1024, a / 1024, f / 1024}' "/proc/$pid/status"
    fi
}
ticks() { awk '{print $14 + $15}' "/proc/$pid/stat"; }

echo "smhubd pid $pid, $(readlink /proc/$pid/exe), $(ls -la /opt/smhub-services/bin/smhubd | awk '{print $5}') bytes"
echo "threads: $(grep Threads /proc/$pid/status | awk '{print $2}')"
echo "private memory now: $(mem); RSS $(grep VmRSS /proc/$pid/status | awk '{print $2, $3}'); peak RSS $(grep VmHWM /proc/$pid/status | awk '{print $2, $3}')"

# response time of a simple GET on the device, straight to the daemon and through nginx
for url in http://127.0.0.1:8000/api/v2/health http://127.0.0.1/api/v2/health http://127.0.0.1/api/v2/system; do
    times=""
    i=0
    while [ "$i" -lt 20 ]; do
        times="$times $(curl -s -o /dev/null -w '%{time_total}' "$url")"
        i=$((i + 1))
    done
    echo "$url: median $(echo "$times" | tr ' ' '\n' | grep . | sort -n | awk '{a[NR]=$1} END {printf "%.1f ms", a[int((NR+1)/2)] * 1000}') of 20"
done

t0=$(ticks)
echo "sampling idle CPU for $IDLE s..."
sleep "$IDLE"
t1=$(ticks)
awk -v a="$t0" -v b="$t1" -v s="$IDLE" -v hz="$hz" 'BEGIN {printf "idle CPU: %d ticks in %d s = %.3f %%\n", b - a, s, (b - a) / hz / s * 100}'
echo "threads after idle: $(grep Threads /proc/$pid/status | awk '{print $2}'); private memory after idle: $(mem)"
