#!/bin/sh
# Compare the service states smhubd reads itself (`smhubd --services`) with what `rc-status`
# says, on a live hub, as root. Every service rc-status lists must have the same state; a
# service it does not list must read as stopped (the API treats a missing service as stopped).
#   services_compare.sh [smhubd binary, default /opt/smhub-services/bin/smhubd]
# Exit status 1 on any difference.
set -u
BIN=${1:-/opt/smhub-services/bin/smhubd}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

up() { cut -d' ' -f1 /proc/uptime; }

a=$(up)
rc-status -a -f ini >"$dir/rc.ini"
b=$(up)
"$BIN" --services >"$dir/own.txt"
c=$(up)

# rc-status words to the API's states: "scheduled" is stopped; "failed" and "unsupervised" unknown
awk -F' = ' '/^\[/ {next} NF >= 2 {
    split($2, w, " "); s = w[1]
    if (s == "" ) s = "stopped"
    else if (s == "scheduled") s = "stopped"
    else if (s !~ /^(started|stopped|starting|stopping|crashed|inactive)$/) s = "unknown"
    print $1, s
}' "$dir/rc.ini" | sort -u >"$dir/rc.txt"
awk -F' = ' '{print $1, $2}' "$dir/own.txt" | sort >"$dir/own.sorted"

echo "rc-status -a: $(echo "$b - $a" | awk '{print ($1 - $3) * 1000}' FS=' ') ms, $(wc -l <"$dir/rc.txt") services listed"
echo "smhubd --services: $(echo "$c - $b" | awk '{print ($1 - $3) * 1000}' FS=' ') ms (whole process), $(wc -l <"$dir/own.sorted") services"

awk 'NR == FNR {rc[$1] = $2; next} {own[$1] = $2}
END {
    bad = 0
    for (n in rc) {
        if (!(n in own)) { print "missing: " n " (rc-status: " rc[n] ")"; bad = 1 }
        else if (own[n] != rc[n]) { print "differs: " n ": rc-status " rc[n] ", smhubd " own[n]; bad = 1 }
    }
    for (n in own) if (!(n in rc) && own[n] != "stopped") { print "extra: " n " is " own[n] " but rc-status does not list it"; bad = 1 }
    if (!bad) print "same state for every service"
    exit bad
}' "$dir/rc.txt" "$dir/own.sorted"
