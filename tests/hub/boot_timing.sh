#!/bin/bash
# Restart the hub and time, from this machine, what its user sees: when the web page answers
# again and when the backend answers with data. Then ask the hub when its kernel and its backend
# process started, so every time can be given from the kernel start as well.
#   boot_timing.sh HUB_ADDRESS API_PATH [ssh helper]
# The ssh helper (or SMHUB_SSH) runs `--root COMMAND` as root on the hub; use the hub's local
# address, because a VPN comes up late in a boot.
# Example: SMHUB_SSH=./my-hub-ssh.sh boot_timing.sh <hub address> /api/v2/system
# API_PATH is fetched through the hub's web server (port 80), as the page fetches it.
set -u
HUB=$1
API=$2
SSH=${3:-${SMHUB_SSH:?set SMHUB_SSH to a helper that runs --root COMMAND on the hub}}
ms() { date +%s%3N; }

t0=$(ms)
# not waited for: the session hangs when the hub goes down under it
"$SSH" --root 'reboot' >/dev/null 2>&1 &
sshpid=$!
down="" ui="" api=""
while :; do
    el=$(( $(ms) - t0 ))
    [ "$el" -gt 600000 ] && { echo "gave up after 600 s"; break; }
    code=$(curl -s -o /dev/null -m 1 -w '%{http_code}' "http://$HUB/")
    if [ -z "$down" ]; then
        [ "$code" != 200 ] && down=$el
    else
        [ -z "$ui" ] && [ "$code" = 200 ] && ui=$el
        if [ -n "$ui" ] && [ -z "$api" ]; then
            c=$(curl -s -o /dev/null -m 2 -w '%{http_code}' "http://$HUB$API")
            [ "$c" = 200 ] && api=$el
        fi
        [ -n "$api" ] && break
    fi
    sleep 0.3
done
kill "$sshpid" 2>/dev/null

# the hub's clock has no battery, so its kernel start is placed on this machine's clock
for i in $(seq 1 60); do
    q=$(ms)
    info=$("$SSH" --root 'cut -d" " -f1 /proc/uptime; for d in /proc/[0-9]*; do e=$(readlink $d/exe 2>/dev/null); case "$e" in */smhubd|*/python3*) tr "\0" " " < $d/cmdline | grep -q -E "smhubd|smhub_backend|smhub-services|uvicorn" && awk "{print \$22}" $d/stat;; esac; done | sort -n | head -1' 2>/dev/null) && break
    sleep 2
done
r=$(ms)
uptime_s=$(echo "$info" | sed -n 1p)
backend_ticks=$(echo "$info" | sed -n 2p)
kernel=$(awk -v q="$q" -v r="$r" -v u="$uptime_s" -v t0="$t0" 'BEGIN {printf "%d", (q + r) / 2 - u * 1000 - t0}')

awk -v down="$down" -v ui="$ui" -v api="$api" -v k="$kernel" -v bt="${backend_ticks:-0}" 'BEGIN {
    printf "from the restart command: page gone %.1f s, kernel started %.1f s, page back %.1f s, backend answers %.1f s\n", down / 1000, k / 1000, ui / 1000, api / 1000
    printf "from the kernel start: backend process started %.1f s, page back %.1f s, backend answers with data %.1f s\n", bt / 100, (ui - k) / 1000, (api - k) / 1000
}'
