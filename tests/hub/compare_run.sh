#!/bin/bash
# The same measurements for whichever version a hub runs, from this machine, for side-by-side
# comparison. Start it at the same point after a boot for each version (it waits until the hub
# has been up MIN_UPTIME seconds). Output goes to out/compare/LABEL.txt and LABEL-*.jpg.
#   SMHUB_HOST=<hub address> SMHUB_SSH=<helper> compare_run.sh LABEL READY_TEXT MIN_UPTIME PATH...
# The helper runs `--root COMMAND` as root on the hub and copies a file with `--put FILE DEST`.
# PATHs are backend routes timed on the hub, straight to port 8000.
set -u
LABEL=$1 READY=$2 MIN_UP=$3
shift 3
HUB=${SMHUB_HOST:?set SMHUB_HOST to the hub address}
here=$(cd "$(dirname "$0")" && pwd)
SSH=${SMHUB_SSH:?set SMHUB_SSH to a helper that runs --root COMMAND and --put FILE DEST on the hub}
OUT=$here/../../out/compare
UI=$here/../../ui
mkdir -p "$OUT"
log=$OUT/$LABEL.txt
: >"$log"
say() { echo "$*" | tee -a "$log"; }
hub() { "$SSH" --root "$@" 2>/dev/null; }

"$SSH" --put "$here/compare.sh" compare.sh >/dev/null 2>&1
hub 'mv /home/smlight/compare.sh /mnt/user/smhub-new/compare.sh'
C="sh /mnt/user/smhub-new/compare.sh"

up=$(hub 'cut -d" " -f1 /proc/uptime' | cut -d. -f1)
if [ "$up" -lt "$MIN_UP" ]; then
    say "waiting $((MIN_UP - up)) s until the hub has been up $MIN_UP s"
    sleep $((MIN_UP - up))
fi

say "== $LABEL: hub snapshot at $(hub 'cut -d" " -f1 /proc/uptime') s uptime"
hub "$C snapshot" | tee -a "$log"
hub 'opkg list-installed | grep -E "^smhub-(services|ui) "; rc-status -a -f ini 2>/dev/null | grep -E "^(zigbee2mqtt|zeesm|matterbridge|node-red|zwave-js-ui|tailscale|mosquitto) " | sort -u' | tee -a "$log"

say "== first visit after boot, empty browser cache"
t=$(hub "$C ticks")
(cd "$UI" && node tests/compare-load.mjs "http://$HUB/" "$LABEL-first" "$READY" --runs 1) | tee -a "$log"
t2=$(hub "$C ticks")
awk -v a="$t" -v b="$t2" 'BEGIN {split(a, x, " "); split(b, y, " "); printf "hub CPU for that visit: backend %.2f s, whole hub %.2f s busy\n", (y[1] - x[1]) / 100, (y[2] - x[2]) / 100}' | tee -a "$log"

say "== five more visits, empty cache each, desktop speed; the last stays open 60 s"
t=$(hub "$C ticks")
(cd "$UI" && node tests/compare-load.mjs "http://$HUB/" "$LABEL" "$READY" --runs 5 --hold 60) | tee -a "$log"
t2=$(hub "$C ticks")
awk -v a="$t" -v b="$t2" 'BEGIN {split(a, x, " "); split(b, y, " "); printf "hub CPU for 5 visits plus 60 s open: backend %.2f s, whole hub %.2f s busy\n", (y[1] - x[1]) / 100, (y[2] - x[2]) / 100}' | tee -a "$log"

say "== three visits with the browser CPU slowed 4x (about a mid-range phone)"
(cd "$UI" && node tests/compare-load.mjs "http://$HUB/" "$LABEL-phone" "$READY" --runs 3 --cpu 4) | tee -a "$log"

say "== screenshots after 0.5, 1, 2, 3, 5 s"
(cd "$UI" && node tests/compare-load.mjs "http://$HUB/" "$LABEL" --film "$OUT") | tee -a "$log"

say "== backend routes on the hub"
hub "$C latency $*" | tee -a "$log"

say "== idle, no browser, 300 s"
hub "$C idle 300" | tee -a "$log"
hub "$C snapshot" | tee -a "$log"
say "== done"
