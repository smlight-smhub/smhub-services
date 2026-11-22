#!/bin/sh
# Build the two packages into out/:
#   smhub-services_<version>_riscv64.ipk   smhubd, the radio flash helper, package scripts
#   smhub-ui_<version>_all.ipk             the web UI
#
# Runs the whole build in the builder image (packaging/Dockerfile.build), so the
# only thing the machine needs is Docker:
#
#   packaging/build-ipk.sh            both packages
#   packaging/build-ipk.sh services   only smhub-services
#   packaging/build-ipk.sh ui         only smhub-ui
#
# SEED_DB is the database a new install starts with; by default the one the
# 1.x package ships (sys-builder), so a new hub gets the same factory account
# and app rows as before.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
what="${1:-all}"
seed="${SEED_DB:-$here/../sys-builder/buildroot-external/package/smhub-services/CONTROL/db}"

if [ "${IN_BUILDER:-}" != 1 ]; then
    [ -f "$seed" ] || { echo "seed database not found: $seed (set SEED_DB)" >&2; exit 1; }
    mkdir -p "$here/out"
    cp "$seed" "$here/out/.seed.db"
    exec "$here/packaging/cargo.sh" env IN_BUILDER=1 sh packaging/build-ipk.sh "$what"
fi

# --- inside the builder; the repository is the current directory ---
set -x
out=out
work=/tmp/ipk
rm -rf "$work"
mkdir -p "$work" "$out"

stamp=$(date +%s)  # file times: nginx derives ETags from them, so each build differs
field() { sed -n "s/^$1:[[:space:]]*//p" "$2" | head -n 1; }

# ar archive with the members opkg expects, in this order
pack() { # <name> <control dir> <data dir> <uid:gid of data>
    name=$1 ctl=$2 data=$3 owner=$4
    v=$(field Version "$ctl/control")
    a=$(field Architecture "$ctl/control")
    size=$(du -sk "$data" | cut -f1)
    sed -i "/^Installed-Size:/d" "$ctl/control"
    echo "Installed-Size: $((size * 1024))" >> "$ctl/control"
    p="$work/pkg-$name"
    rm -rf "$p" && mkdir -p "$p"
    printf '2.0\n' > "$p/debian-binary"
    tar --format=gnu --sort=name --numeric-owner --owner=0 --group=0 --mtime=@"$stamp" \
        -C "$ctl" -czf "$p/control.tar.gz" .
    tar --format=gnu --sort=name --numeric-owner --owner="${owner%%:*}" --group="${owner##*:}" --mtime=@"$stamp" \
        -C "$data" -czf "$p/data.tar.gz" $(cd "$data" && ls -A | sed 's|^|./|')  # no "./" entry: opkg would list / as a package file
    rm -f "$out/${name}_${v}_${a}.ipk"
    (cd "$p" && ar -cr "$OLDPWD/$out/${name}_${v}_${a}.ipk" debian-binary control.tar.gz data.tar.gz)
    ls -la "$out/${name}_${v}_${a}.ipk"
}

if [ "$what" = all ] || [ "$what" = services ]; then
    cargo zigbuild --release --target riscv64gc-unknown-linux-musl -p smhubd
    ctl="$work/services-ctl" data="$work/services-data"
    mkdir -p "$ctl" "$data/opt/smhub-services/bin" "$data/opt/smhub-services/flasher/lib" "$data/opt/smhub-services/overlays" "$data/opt/bin"
    # device-tree overlays the daemon copies to the boot partition when a setting needs them
    for d in packaging/smhub-services/overlays/*.dts; do
        dtc -@ -q -I dts -O dtb -o "$data/opt/smhub-services/overlays/$(basename "$d" .dts).dtbo" "$d"
    done
    cp packaging/smhub-services/CONTROL/* "$ctl/"
    cp "$out/.seed.db" "$ctl/db"
    chmod 0755 "$ctl/openrc" "$ctl/postinst" "$ctl/prerm" "$ctl/postrm"
    install -m 0755 "$CARGO_TARGET_DIR/riscv64gc-unknown-linux-musl/release/smhubd" "$data/opt/smhub-services/bin/smhubd"
    install -m 0755 helpers/radio-flash/radio-flash.py "$data/opt/smhub-services/flasher/radio-flash.py"
    tar -xzf packaging/vendor/flasher-lib-py314-riscv64.tgz -C "$data/opt/smhub-services/flasher/lib"
    find "$data/opt/smhub-services/flasher/lib" -name '*.dist-info' -prune -o -name tests -type d -prune -exec rm -rf {} + 2>/dev/null || true
    chmod -R u+rwX,go+rX,go-w "$data/opt/smhub-services"
    ln -s /opt/smhub-services/bin/smhubd "$data/opt/bin/smhub-services"
    pack smhub-services "$ctl" "$data" 0:0
fi

if [ "$what" = all ] || [ "$what" = ui ]; then
    # build from a copy: ui/node_modules on the host belongs to the host's platform
    rm -rf "$work/ui-src" && mkdir -p "$work/ui-src"
    tar -C ui --exclude=./node_modules --exclude=./dist -cf - . | tar -C "$work/ui-src" -xf -
    # the UI compares this with System.ui_version and reloads when a cached copy is older
    ui_version=$(field Version packaging/smhub-ui/CONTROL/control)
    (cd "$work/ui-src" && npm ci --no-audit --no-fund && VITE_UI_VERSION="$ui_version" npm run build)
    ctl="$work/ui-ctl" data="$work/ui-data"
    mkdir -p "$ctl" "$data/opt/smhub-ui"
    cp packaging/smhub-ui/CONTROL/* "$ctl/"
    # no postinst: the package itself gives the files to www-data, readable by all
    cp -r "$work/ui-src/dist/." "$data/opt/smhub-ui/"
    chmod -R u+rwX,go+rX,go-w "$data/opt/smhub-ui"
    pack smhub-ui "$ctl" "$data" 33:33
fi
rm -f "$out/.seed.db"
