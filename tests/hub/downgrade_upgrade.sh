#!/bin/sh
# Go back from 2.x to the 1.x packages and forward again, on a live hub, as root.
#
#   downgrade_upgrade.sh <dir with the 1.x ipks> <dir with the 2.x ipks>
#
# Checks: the 1.x backend starts on the database 2.x used and answers its API;
# the database keeps its rows; going forward again works and the rows are kept;
# an account and a backup made by 1.x work on 2.x.
#
# Run it detached (nohup ... &): the 1.x backup stops every app service,
# Tailscale included, which drops a session that came in over Tailscale.
set -u
OLD=$1
NEW=$2
DB=/opt/smhub-services/config/backend.db
fail=0
say() { echo "$(date +%T) $*"; }
check() { if eval "$2"; then say "PASS $1"; else say "FAIL $1"; fail=1; fi; }

wait_api() { # <seconds>: until the readiness address answers at all
    i=0
    while [ "$i" -lt "$1" ]; do
        c=$(curl -s -o /dev/null -m 3 -w '%{http_code}' http://127.0.0.1:8000/api/v1/login/user-session)
        [ "$c" = 401 ] || [ "$c" = 200 ] && { echo "$i"; return 0; }
        sleep 2; i=$((i + 2))
    done
    echo "$i"; return 1
}

rows() { sqlite3 "$DB" "select (select count(*) from apps)||'/'||(select count(*) from appsettings)||'/'||(select count(*) from devicesettings)||'/'||(select count(*) from user)||'/'||(select count(*) from configsource)"; }

say "before: $(opkg list-installed | grep -E '^smhub-(services|ui) ') rows=$(rows)"
before=$(rows)
sqlite3 "$DB" .dump > /mnt/user/smhub-backup/db-before-downgrade.sql

say "going back to 1.x"
opkg install --force-downgrade "$OLD"/smhub-services_1.*_riscv64.ipk "$OLD"/smhub-ui_1.*_all.ipk > /tmp/downgrade.log 2>&1
check "opkg downgrade" "[ $? -eq 0 ]"
tail -3 /tmp/downgrade.log
check "1.x installed" "opkg list-installed | grep -q '^smhub-services - 1\.'"
t=$(wait_api 240); check "1.x backend answers (after ${t} s)" "[ $? -eq 0 ]"
check "1.x API reads the security page" "curl -s -m 10 http://127.0.0.1:8000/api/v1/login/security-status | grep -q enabled"
check "1.x UI files served" "curl -s -m 5 http://127.0.0.1/ | grep -q fonts.googleapis"
after_down=$(rows)
check "rows kept after going back ($before -> $after_down)" "[ '$before' = '$after_down' ]"

# made by 1.x, used by 2.x later: an account (its password hash) and a backup
S=/run/smhub-backend.sock
code=$(curl -s -o /tmp/signup.out -w '%{http_code}' --unix-socket "$S" -H 'Content-Type: application/json' \
    -d '{"email":"legacy-test@example.com","password":"Legacy-test-1","full_name":null}' http://localhost/api/v1/users/signup)
check "1.x creates an account (status $code)" "[ '$code' = 200 ] || [ '$code' = 201 ]"
code=$(curl -s -o /mnt/user/smhub-backup/backup-made-by-1x.tar.xz -w '%{http_code}' --unix-socket "$S" -X POST \
    -H 'Content-Type: application/json' -d '{"exclude_home":true}' http://localhost/api/v1/stream/create-backup)
check "1.x makes a backup (status $code, $(wc -c < /mnt/user/smhub-backup/backup-made-by-1x.tar.xz) bytes)" "[ '$code' = 200 ]"
added=$(rows)
sqlite3 "$DB" .dump > /mnt/user/smhub-backup/db-after-downgrade.sql

say "forward to 2.x again"
opkg install "$NEW"/smhub-services_2.*_riscv64.ipk "$NEW"/smhub-ui_2.*_all.ipk > /tmp/upgrade.log 2>&1
check "opkg upgrade" "[ $? -eq 0 ]"
tail -3 /tmp/upgrade.log
t=$(wait_api 60); check "2.x answers (after ${t} s)" "[ $? -eq 0 ]"
check "2.x API" "curl -s -m 5 http://127.0.0.1/api/v2/health | grep -q ok"
after_up=$(rows)
check "rows kept after going forward ($added -> $after_up)" "[ '$added' = '$after_up' ]"
check "old Python environment removed" "[ ! -d /opt/smhub-services/venv/lib ]"

code=$(curl -s -o /tmp/signin.out -w '%{http_code}' -H 'Content-Type: application/json' \
    -d '{"username":"legacy-test@example.com","password":"Legacy-test-1"}' http://127.0.0.1/api/v2/session)
check "the account made by 1.x signs in on 2.x (status $code)" "[ '$code' = 200 ]"
uid=$(sqlite3 "$DB" "select id from user where email = 'legacy-test@example.com'")
[ -n "$uid" ] && curl -s -o /dev/null -X DELETE "http://127.0.0.1/api/v2/users/$uid"
left=$(sqlite3 "$DB" "select count(*) from user where email = 'legacy-test@example.com'")
check "test account removed" "[ '$left' = 0 ]"

# check the 1.x backup with 2.x (dry run: nothing is restored)
size=$(wc -c < /mnt/user/smhub-backup/backup-made-by-1x.tar.xz)
up=$(curl -s -H 'Content-Type: application/json' -d "{\"size\":$size,\"name\":\"backup.tar.xz\"}" http://127.0.0.1/api/v2/uploads | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
# pieces of 512 KiB (busybox has no split)
n=0
while [ $((n * 524288)) -lt "$size" ]; do
    dd if=/mnt/user/smhub-backup/backup-made-by-1x.tar.xz of=/tmp/bk-piece bs=524288 skip="$n" count=1 2>/dev/null
    curl -s -o /dev/null -X PUT --data-binary @/tmp/bk-piece -H 'Content-Type: application/octet-stream' "http://127.0.0.1/api/v2/uploads/$up?offset=$((n * 524288))"
    n=$((n + 1))
done
rm -f /tmp/bk-piece
curl -s -H 'Content-Type: application/json' -d "{\"upload_id\":\"$up\",\"dry_run\":true}" http://127.0.0.1/api/v2/backups/restore > /tmp/restore-check.out
check "2.x reads the backup made by 1.x: $(head -c 160 /tmp/restore-check.out)" "grep -q same_device /tmp/restore-check.out"
sqlite3 "$DB" .dump > /tmp/db-after-upgrade.sql
diff /mnt/user/smhub-backup/db-after-downgrade.sql /tmp/db-after-upgrade.sql > /tmp/db-roundtrip.diff 2>/dev/null || true
say "database changes over the round trip: $(grep -c '^[-+][^-+]' /tmp/db-roundtrip.diff 2>/dev/null || echo 0) lines"
[ "$fail" -eq 0 ] && say "ALL PASSED" || say "SOME FAILED"
exit "$fail"
