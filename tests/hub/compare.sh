#!/bin/sh
# Measure the hub's core backend, whichever is installed (1.x Python or 2.0 smhubd), the same
# way for both, so the two can be compared on one hub. Run as root on the hub.
#   compare.sh snapshot              version, memory, threads, CPU used so far, whole system
#   compare.sh idle SECONDS          backend and whole-system CPU over a quiet window
#   compare.sh latency PATH...       median of 20 requests straight to the backend (port 8000)
#   compare.sh ticks                 backend CPU ticks and system busy/total ticks now
set -u
hz=100

ver=$(opkg list-installed 2>/dev/null | awk '$1 == "smhub-services" {print $3}')

# The backend process: smhubd, or the Python process that runs smhub_backend (the biggest one).
pid=""
best=0
for d in /proc/[0-9]*; do
    exe=$(readlink "$d/exe" 2>/dev/null) || continue
    case "$exe" in
        */smhubd) ;;
        */python3*) tr '\0' ' ' <"$d/cmdline" 2>/dev/null | grep -q -E 'smhub_backend|smhub-services|uvicorn' || continue ;;
        *) continue ;;
    esac
    rss=$(awk '/^VmRSS/ {print $2}' "$d/status" 2>/dev/null)
    [ "${rss:-0}" -gt "$best" ] && best=$rss && pid=${d#/proc/}
done
[ -n "$pid" ] || { echo "no backend process found (version $ver)"; exit 1; }

# utime+stime, and the same for children it waited for (commands it ran, such as rc-status)
own_ticks() { awk '{print $14 + $15}' "/proc/$pid/stat"; }
all_ticks() { awk '{print $14 + $15 + $16 + $17}' "/proc/$pid/stat"; }
sys_busy() { awk '/^cpu / {print $2 + $3 + $4 + $7 + $8 + $9}' /proc/stat; }
sys_total() { awk '/^cpu / {print $2 + $3 + $4 + $5 + $6 + $7 + $8 + $9}' /proc/stat; }
kib() { awk -v k="$1" '$1 == k":" {print $2}' "/proc/$pid/status"; }

case "${1:-snapshot}" in
snapshot)
    up=$(cut -d' ' -f1 /proc/uptime)
    start=$(awk -v hz=$hz '{printf "%.1f", $22 / hz}' "/proc/$pid/stat")
    private_all=$(for s in /proc/[0-9]*/status; do awk '/^RssAnon/ {print $2}' "$s" 2>/dev/null; done | awk '{s += $1} END {print s}')
    awk -v ver="$ver" -v pid="$pid" -v up="$up" -v start="$start" \
        -v anon="$(kib RssAnon)" -v file="$(kib RssFile)" -v rss="$(kib VmRSS)" -v hwm="$(kib VmHWM)" \
        -v thr="$(kib Threads)" -v own="$(own_ticks)" -v all="$(all_ticks)" -v hz=$hz -v pall="$private_all" '
    BEGIN {
        printf "version=%s pid=%s uptime_s=%.0f backend_started_at_s=%s backend_age_s=%.0f\n", ver, pid, up, start, up - start
        printf "backend_private_mib=%.1f backend_rss_mib=%.1f backend_file_mib=%.1f backend_peak_rss_mib=%.1f threads=%s\n", anon / 1024, rss / 1024, file / 1024, hwm / 1024, thr
        printf "backend_cpu_s=%.1f backend_cpu_with_children_s=%.1f\n", own / hz, all / hz
        printf "all_processes_private_mib=%.1f\n", pall / 1024
    }'
    awk '/^MemTotal/ {t = $2} /^MemAvailable/ {a = $2} END {printf "system_used_mib=%.0f system_total_mib=%.0f\n", (t - a) / 1024, t / 1024}' /proc/meminfo
    echo "load=$(cut -d' ' -f1-3 /proc/loadavg)"
    ;;
idle)
    # own ticks, and children's separately: a child's CPU is counted only when it exits, so a
    # long-running helper started earlier lands in whatever window it happens to end in
    s=${2:-300}
    o0=$(own_ticks); a0=$(all_ticks); b0=$(sys_busy); t0=$(sys_total)
    sleep "$s"
    o1=$(own_ticks); a1=$(all_ticks); b1=$(sys_busy); t1=$(sys_total)
    awk -v o="$((o1 - o0))" -v c="$((a1 - a0 - o1 + o0))" -v b="$((b1 - b0))" -v t="$((t1 - t0))" -v s="$s" -v hz=$hz 'BEGIN {
        printf "idle_window_s=%d backend_own_ticks=%d backend_own_cpu_pct=%.3f children_ticks=%d system_busy_pct=%.2f\n", s, o, o / hz / s * 100, c, b / t * 100 }'
    echo "threads_after=$(kib Threads) backend_private_mib_after=$(kib RssAnon | awk '{printf "%.1f", $1 / 1024}')"
    ;;
latency)
    shift
    for path in "$@"; do
        times=""
        code=""
        i=0
        while [ "$i" -lt 20 ]; do
            r=$(curl -s -o /dev/null -m 10 -w '%{http_code} %{time_total}' "http://127.0.0.1:8000$path")
            code=${r% *}
            times="$times ${r#* }"
            i=$((i + 1))
        done
        echo "$times" | tr ' ' '\n' | grep . | sort -n | awk -v p="$path" -v c="$code" '{a[NR] = $1} END {
            printf "latency path=%s status=%s median_ms=%.1f min_ms=%.1f max_ms=%.1f\n", p, c, a[int((NR + 1) / 2)] * 1000, a[1] * 1000, a[NR] * 1000 }'
    done
    ;;
ticks)
    echo "$(all_ticks) $(sys_busy) $(sys_total)"
    ;;
esac
