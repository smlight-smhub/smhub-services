#!/bin/sh
# Install the newest release of this repository from GitHub, pre-releases included
# (https://github.com/smlight-smhub/smhub-services/releases): its two packages,
# smhub-services and smhub-ui.
# On the hub, in any terminal:
#   curl -fsSL https://raw.githubusercontent.com/smlight-smhub/smhub-services/main/packaging/update-beta.sh | sh
# Started as a user, it runs again through sudo, which asks for that user's password.
set -e
trap '' HUP   # the web terminal may close while smhub-services restarts; keep going
REPO=smlight-smhub/smhub-services
SELF=https://raw.githubusercontent.com/$REPO/main/packaging/update-beta.sh
if [ "$(id -u)" != 0 ]; then
    command -v sudo > /dev/null || { echo "Run this as root." >&2; exit 1; }
    echo "Installing needs root: enter your password for sudo."
    exec sudo sh -c "curl -fsSL $SELF | sh"
fi
dir=/tmp/smhub-update log=/tmp/smhub-update.log
rm -rf "$dir" "$log"
mkdir "$dir"
cd "$dir"
echo "Looking for the newest release..."
urls=$(curl -fsSL -H "Accept: application/vnd.github+json" \
        "https://api.github.com/repos/$REPO/releases?per_page=1" |
    grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(https:[^"]*\)"$/\1/')
services=$(echo "$urls" | grep '/smhub-services_[^/]*_riscv64\.ipk$' | head -n 1)
ui=$(echo "$urls" | grep '/smhub-ui_[^/]*_all\.ipk$' | head -n 1)
if [ -z "$services" ] || [ -z "$ui" ]; then
    echo "The newest release at https://github.com/$REPO/releases does not have both packages." >&2
    exit 1
fi
services_file=${services##*/} ui_file=${ui##*/}
echo "Downloading $services_file and $ui_file..."
curl -fsSL -o "$services_file" "$services"
curl -fsSL -o "$ui_file" "$ui"
echo "Installing (log: $log)..."
# --force-downgrade: this release is what was asked for, even on a hub with a build that sorts
# after it, such as a package built from a later commit.
if ! opkg install --force-downgrade "./$services_file" "./$ui_file" > "$log" 2>&1; then
    tail -5 "$log"; exit 1
fi
cd /
rm -rf "$dir"
rc-service smhub-services status > /dev/null 2>&1 || rc-service smhub-services start
# The package version (2.0.0~beta.1-1) as people write it (2.0.0-beta.1).
version=$(opkg list-installed | sed -n 's/^smhub-services - //p' | sed 's/~/-/; s/-[0-9]*$//')
echo "Done: smhub-services and smhub-ui $version. Reload the hub's page in the browser."
