#!/bin/sh
# Put temporary OpenRC services through every state smhubd reports (starting, started, stopping,
# crashed, unsupervised) and compare its reading with rc-status at each step, on a live hub, as
# root. The test services are removed at the end.
#   services_states.sh [smhubd binary] [services_compare.sh]
set -u
BIN=${1:-/opt/smhub-services/bin/smhubd}
CMP=${2:-$(dirname "$0")/services_compare.sh}
fail=0

check() { # step name, service, expected API state
    got=$("$BIN" --services 2>/dev/null | awk -v n="$2" '$1 == n {print $3}')
    if sh "$CMP" "$BIN" >/tmp/smhub-cmp.$$ 2>&1 && [ "$got" = "$3" ]; then
        echo "PASS $1: $2 is $got, as rc-status says"
    else
        echo "FAIL $1: $2 is '$got' (want $3)"; sed 's/^/    /' /tmp/smhub-cmp.$$; fail=1
    fi
    rm -f /tmp/smhub-cmp.$$
}

cleanup() { # by pid, not pkill -f: that would also match a shell whose command line names the test
    child=$(cat /run/openrc/options/smhub-test-sup/child_pid 2>/dev/null)
    [ -n "$child" ] && kill "$child" 2>/dev/null
    [ -f /run/smhub-test-ssd.pid ] && kill "$(cat /run/smhub-test-ssd.pid)" 2>/dev/null
    [ -f /run/smhub-test-sup.pid ] && kill "$(cat /run/smhub-test-sup.pid)" 2>/dev/null
    for s in smhub-test-slow smhub-test-ssd smhub-test-sup; do
        rc-service "$s" zap >/dev/null 2>&1
        rm -f "/etc/init.d/$s"
    done
    rm -f /run/smhub-test-ssd.pid /run/smhub-test-sup.pid
}
trap cleanup EXIT

cat >/etc/init.d/smhub-test-slow <<'EOF'
#!/sbin/openrc-run
description="smhubd state test: slow start and stop (temporary)"
start() { ebegin "starting slowly"; sleep 4; eend 0; }
stop() { ebegin "stopping slowly"; sleep 4; eend 0; }
EOF
cat >/etc/init.d/smhub-test-ssd <<'EOF'
#!/sbin/openrc-run
description="smhubd state test: start-stop-daemon with a pidfile (temporary)"
command=/bin/sleep
command_args=4242
command_background=yes
pidfile=/run/smhub-test-ssd.pid
EOF
cat >/etc/init.d/smhub-test-sup <<'EOF'
#!/sbin/openrc-run
description="smhubd state test: supervise-daemon (temporary)"
supervisor=supervise-daemon
command=/bin/sleep
command_args=4242
pidfile=/run/smhub-test-sup.pid
respawn_max=0
EOF
chmod 0755 /etc/init.d/smhub-test-slow /etc/init.d/smhub-test-ssd /etc/init.d/smhub-test-sup

check "never started" smhub-test-slow stopped
rc-service smhub-test-slow start >/dev/null 2>&1 &
sleep 1.5
check "while starting" smhub-test-slow starting
wait
check "after start" smhub-test-slow started
rc-service smhub-test-slow stop >/dev/null 2>&1 &
sleep 1.5
check "while stopping" smhub-test-slow stopping
wait
check "after stop" smhub-test-slow stopped

rc-service smhub-test-ssd start >/dev/null 2>&1
check "daemon running" smhub-test-ssd started
kill "$(cat /run/smhub-test-ssd.pid)"
sleep 0.5
check "daemon killed" smhub-test-ssd crashed
rm -f /run/smhub-test-ssd.pid
check "pidfile gone" smhub-test-ssd crashed

rc-service smhub-test-sup start >/dev/null 2>&1
sleep 1
check "supervised" smhub-test-sup started
kill -9 "$(cat /run/smhub-test-sup.pid)"
sleep 0.5
check "supervisor killed" smhub-test-sup unknown

[ "$fail" = 0 ] && echo "ALL PASSED" || echo "SOME FAILED"
exit "$fail"
