# Verification

How `smhub-services` and `smhub-ui` 2.0 (version 2.0.3, package version
2.0.3-1) are checked, and what holds today: the checks,
their results on the development hub, what 2.0 covers of 1.x and what it does
not, known issues, what is not verified, and the state of the translations.

The results come from one hub, **the development hub**: SMHUB Essential,
hardware 1.01, OS 1.0.2, one 1 GHz RISC-V core, 488 MiB RAM, the ESPHome
firmware `esphome-bin` 2026.5.3-5 on the second core. Its 1.x packages are
`smhub-services` 1.1.0-1 and `smhub-ui` 1.0.6-1. The browser is Chromium,
over the local network or over Tailscale, as each result says.

## The checks

### On a development machine

| Check | Command | Covers |
|---|---|---|
| Rust unit tests | `packaging/cargo.sh cargo test --workspace` | tests next to the code in every crate: parsers and config writers (ser2net, Mosquitto, opkg status, NetworkManager, stunnel, app settings), the 1.x database, password hashes and the factory account (D41), app forms with read-only fields, the hardware modules (light effects, button gestures, infrared decoders and learning, RTTTL) |
| Rust lints | `packaging/cargo.sh cargo clippy --workspace --all-targets` | no warnings |
| UI types | `cd ui && npm run typecheck` | the UI and the browser tests against the types generated from `api/openapi.yaml`; a missing English text fails it |
| UI build | `cd ui && npm run build` | the build, then `scripts/size-report.mjs` (fails when the first load is over 400 KB) and `scripts/i18n-report.mjs` (missing keys and placeholders per language) |
| Translations | `cd ui && node scripts/check-locale.mjs [code ...]` | each language file has exactly English's keys, keeps every `{placeholder}`, has no empty text and has exactly the plural forms the browser's rules use for the language (D36) |
| Colour tones | `cd ui && node scripts/tones.mjs` | every colour tone of the Appearance page keeps 4.5:1 contrast for every text pair, light and dark |

### In a browser, against a hub

Each script finds the hub through `SMHUB_URL` or its first argument and puts
back what it changes.

| Script | Covers |
|---|---|
| `ui/tests/pages.spec.ts` (`npm test`, Playwright) | every page at both levels, on a phone (Pixel 7) and a desktop (1366×860), with an axe check for WCAG 2.2 A and AA on each; app pages and their tabs, an app inside the hub, dialogs; old `/#/` addresses, the command palette, the phone's More sheet, a setting saved and undone, the dark theme, Ukrainian; the first-run setup with `SMHUB_TEST_SETUP=1` on a hub in its first-run state |
| `ui/tests/clickthrough.mjs` | every action the page tests leave out, with real clicks and typing in Chromium (see [Pages and actions](#pages-and-actions)); `APPS=a,b` limits its every-app step |
| `ui/tests/stale-state.mjs` | that the live state ends right when a service is stopped or started while other requests keep the hub's status busy |
| `ui/tests/load-timing.mjs` | requests and bytes before the home page is usable, time to usable with an empty and a warm cache, requests while a page stays open |
| `ui/tests/compare-load.mjs` | the same page-load measurement for whichever version a hub serves (1.x or 2.0), with a slowed CPU and a film strip |
| `ui/tests/load-trace.mjs` | when each request of a cold load finishes and each event frame arrives |
| `ui/tests/snapshot.mjs` | the accessibility tree of every page |
| `ui/tests/screenshots.mjs` | the README's pictures, with the hub's addresses replaced by example values |
| `ui/tests/probe.mjs`, `probe-phone.mjs` | ad-hoc probes of one page |

### On the hub

Run as root on the hub; each puts back what it changes.

| Script | Covers |
|---|---|
| `tests/hub/api_write_test.py` | the API's write paths through nginx (see [API write paths](#api-write-paths)) |
| `tests/hub/hardware_test.py [--sound]` | light ring, LEDs, buttons, infrared, API tokens; one short beep with `--sound` |
| `tests/hub/services_compare.sh` | smhubd's own reading of service states against `rc-status`, every service |
| `tests/hub/services_states.sh` | test services through starting, started, stopping, crashed and unsupervised, each compared with `rc-status`, then removed |
| `tests/hub/downgrade_upgrade.sh` | 2.0 → 1.x → 2.0 with data kept; run it detached, because 1.x's backup stops every app service, Tailscale included |
| `tests/hub/measure.sh` | smhubd's memory, threads, idle CPU and request times against the targets in [vision.md](vision.md#targets) |
| `tests/hub/compare.sh`, `compare_run.sh`, `boot_timing.sh` | the same measurements for 1.x and 2.0 on one hub, and restart timing from another machine ([report](report/index.html)) |
| `tests/hub/cpu_sample.py` | the hub's CPU once a second, with short-lived commands counted |

### Not built

- The contract tests and effect tests of D46: no test checks every answer
  against `api/openapi.yaml`, and none compares the files and commands of a
  setting change with the 1.x backend.
- Continuous integration ([plan.md](plan.md#open-questions), question 5). The
  checks above are run by hand.

## Results

Two kinds of build of 2.0 are tested:

- **Release packages:** built by SMLIGHT's package builder from a commit of
  this repository, with its Buildroot toolchain (Rust 1.96.1, crates vendored,
  `--offline --locked`, the size-first profile). The daemon is linked against
  the OS image's glibc. The packages are 5.8 MB (`smhub-services`) and 0.7 MB
  (`smhub-ui`). The UI files carry the source commit's date, so each release
  gets new nginx ETags. The UPS board page is left out.
- **Builder-image builds:** `packaging/build-ipk.sh` in this repository's
  builder image, with a static musl daemon. Some of the measurements below are
  of such a build without the hub's hardware modules (buzzer, light ring, LEDs,
  infrared, buttons, tokens); the table says so.

### Test runs

| Check | Result | Build |
|---|---|---|
| `pages.spec.ts` | 146 passed, 4 skipped, 0 failed. Skipped: the first-run setup on both projects (it needs `SMHUB_TEST_SETUP=1` and a new hub; the click-through covers it), the command palette's keyboard shortcut on a phone, the phone navigation on a desktop | release packages |
| `api_write_test.py` | 116 checks, 0 failed | release packages |
| `hardware_test.py` | 31 of 31 | release packages |
| `clickthrough.mjs` | 108 passed, 0 failed, in one complete pass | builder-image build without the Peripherals section and the Regional settings page |
| `downgrade_upgrade.sh` | every check passes | builder-image build without the hardware modules |
| `services_compare.sh` | the same state as `rc-status` for all 98 services, in three runs | builder-image build |
| `services_states.sh` | every state matches `rc-status` | builder-image build |
| `stale-state.mjs` on Zigbee2MQTT | 0 of 20 stop and start rounds leave the live state stale | builder-image build |
| `check-locale.mjs` | every one of the 30 language files passes: 1,413 texts each | the current source |

### Upgrade from 1.x

| Check | Result |
|---|---|
| `opkg install` of 2.0 over 1.1.0-1 | 16 s; the old service is stopped by its own prerm and the new one started by the new postinst |
| `opkg install` of a 2.0 test build (`2.0.0~beta.1-1`) over 1.1.0-1, without `--force` | 15 s; data, apps and the time zone kept |
| `opkg install` of 2.0.0-3 over that test build, and of 2.0.3-1 over 2.0.0-3, without `--force` | 11 s and 10 s; data, apps and the time zone kept; 2.0.3-1 sorts after the feed builds 2.0.0-1 and 2.0.0-2, so no update to them is offered |
| The 1.x Python environment | removed: 145 MB freed |
| `backend.db` after the upgrade | identical except the `apps.installed` and `start_at_boot` corrections of D50 and `updated_at` on those rows |
| App HTTPS ports in stunnel | unchanged (`app-zigbee2mqtt.conf` 28080, `app-zeesm.conf` 28787) |
| The OS image's readiness check (`api-ready`) | passes |
| Restart, and stop and start repeatedly | pass; the init script removes the readiness socket that OpenRC 0.63 leaves behind (D48) |
| Reboot | smhubd ready 325 ms after it starts, about 4 s into boot; `api-ready` passes about 12 s into boot |
| A browser with the 1.x UI in its cache | shows the 1.x sign-in form; its Login button reloads into the new UI (D51) |

### Going back to 1.x and forward again

`tests/hub/downgrade_upgrade.sh`, with the 1.x packages 1.1.0-1 and 1.0.6-1.

| Check | Result |
|---|---|
| `opkg install --force-downgrade` of the 1.x packages over 2.0 | 2.5 min (the 1.x package unpacks 6,800 files) |
| The 1.x backend on the database 2.0 used | answers after 46 s; its API and the 1.x UI work |
| Rows kept going back (apps, app settings, device settings, users, config sources) | 17/18/23/2/5 before and after |
| An account made by the 1.x backend (its own Argon2 hash) | signs in on 2.0 |
| A backup made by the 1.x backend | read by 2.0's restore check: the same device, its apps and data folders listed |
| Forward to 2.0 again | 19 s; answers at once; the Python environment is removed again |
| The database over the round trip | the same, apart from the test's own account (removed by the test) and the D50 `installed` values, which each side writes its own way |

### API write paths

`api_write_test.py` covers: preferences; time zone and host name; the MQTT
port and users (Mosquitto `$7$` hashes, file mode kept); SSH keys; the serial
bridge; ESPHome; USB; backup settings; accounts; sign-in on and off; signing in
with the right and a wrong password; files (folders, an upload of 1.7 MB in
pieces, read, write, move, copy, delete, refusal of paths outside the roots
and of `..`); a backup archive (metadata first, the database inside) and its
check; app settings written to the database and to `configuration.yaml` with a
byte-exact round trip; app flags; service restart; refusal to stop sshd; the
update channel's round trip through the opkg feed file; every listed log
opened and downloaded through nginx; the event connection; the terminal
(commands, resize, runs as `smlight`); following a log; installing and removing
an app (Matterbridge: 22 s and 9 s); the readiness answer and the cached 1.x
UI's sign-in call (D51). Values with `/` in them (an MQTT user `api/test`, an
SSH key whose fingerprint has `/`, a Wi-Fi profile `apitest/slash`) are
removed through query parameters, because the OS image's nginx decodes `%2F`
in a path.

### Pages and actions

`clickthrough.mjs` in Chromium, against the hub's own nginx over Tailscale.

| Area | What is clicked |
|---|---|
| Home | arrange (move a card, hide a card, done), the layout saved and reset; start and stop an app from its card; dismiss the open-hub notice; "Stop Tailscale" asks first and warns that the page comes through Tailscale (cancelled) |
| Shell | sidebar collapse and expand; simple and advanced level from the sidebar and from a page; Ctrl+K, type, Enter |
| Phone (390×844) | bottom bar, the More sheet, no sideways scrolling |
| Apps | check for updates (`opkg update`); install Matterbridge from its page, stop, start, remove; reset a test app's settings; Zigbee2MQTT: watchdog, start with the hub, a setting saved to the database and `configuration.yaml` and put back, log and changelog tabs; Zeesm's own page inside the hub |
| Network | Wi-Fi: scan, radio off and on, join a network that does not exist (a clear message, no profile left); VPN: fill in and save, import a `.conf`, remove (only a test network routed); LTE: refresh, power on, reset the modem |
| Radios | write coordinator firmware 20250325 to the CC2652P from the page (30 s) |
| Updates | check now; the test channel (asks first) and back to stable; automatic updates on and off |
| General | a new host name, then the old one back; time zone searched, chosen and put back |
| Sign-in and accounts | set a password; another browser is refused and shown the sign-in page; a wrong password is refused; sign in; sign out; add an account, change its password, remove it; stop asking for a password |
| Appearance and regional settings | Ukrainian and back; dark theme; imperial units; the theme button's cycle; preferences put back |
| MQTT | add and remove a broker account; port 1884 and back; with Zigbee2MQTT running, it reconnects after each save, and turning the password requirement on asks first (cancelled) |
| ESPHome | buzzer off and on; show the key; make a new key (the factory key put back); restart ESPHome |
| Backups | make and download one (21 KB); choose that file, its details are shown, restore; apps stop and start again, the daemon reopens the database; the database is identical afterwards |
| USB | host and back to device |
| SSH | root sign-in off and on; add and remove a key whose fingerprint contains `/` |
| Serial bridge | share `/dev/ttyS2` on TCP 6638, check that the port accepts connections, start, stop, remove |
| System | power off asks first (cancelled); restart the hub: it answers again after 93 s and the open page reconnects by itself |
| Terminal | a command typed on the page runs as `smlight` |
| Files | new folder, an upload of 1.2 MB in pieces, rename, download (same size), delete the folder |
| Logs | open a log, filter, download |
| Services | restart stunnel (asks first); no Stop for sshd and no actions for the hub's own service; start and stop an app's service |
| First-run setup | on a hub made to look new: language, units, name, time zone, the password step skipped, detail level, "Go to my hub"; not offered again |

The Peripherals pages (Buzzer, Lights, Infrared remote, Buttons, UPS) are not
in the click-through. They are covered by `hardware_test.py` through the API, by `pages.spec.ts`, and by hand in the browser
against the hub: live data, translated effect names, no console errors.

These controls are left out on purpose, because they would cut the hub off or
cannot be undone on it: Tailscale "Log out" and a confirmed "Stop Tailscale";
saving an Ethernet change; installing an OS version; a confirmed power off;
restoring, deleting or downloading a factory backup (the development hub has
none).

### Apps

| App | Result |
|---|---|
| Matterbridge | installs in 22–23 s, starts and keeps running, its web page answers on 8283 (HTTPS 28283); start and stop from its page 1.4 s each; removed |
| Matterbridge Home Assistant and Shelly plugins | install in about 2 minutes each (they pull in Matterbridge), are not services, remove cleanly |
| Matterbridge Zigbee2MQTT plugin | installs and removes |
| Zigbee2MQTT | starts and stops from the home page and from its own page; stays connected through MQTT saves (D53); Stop then Start one second apart both succeed, in order |
| Z-Wave JS UI 11.21.0-1, installed through the API | its six security keys and its `enabled` switch are read-only on the Settings tab ("Set by the app. It can’t be changed here."); a `PATCH` with any of them answers 422 `invalid_value` on that field and leaves `settings.json` unchanged; the writable `port` saves (200) |

While an app starts or stops on its own, its page, its home card and Services
show a disabled "Starting…" or "Stopping…"; the hub waits up to 30 s for the
service to settle before it acts, else answers "busy".

### Radios

| Radio | Result |
|---|---|
| 1, CC2652P, coordinator 20250325 | flashes in 24 s; both image segments verified by the flasher |
| 2, EFR32MG21, factory coordinator 7.4.1.0 | fails, as with 1.x: the radio does not answer its bootloader after the reset pattern ("Failed to probe running application type"). The 1.x backend logs the same error on this hub |
| 3, ZG23 (Z-Wave) | not run: the update server offers no firmware for this chip |

### The hub's hardware

| Check | Result |
|---|---|
| ESPHome client (`smhubd --esphome 8`, ESPHome 2026.5.3 behind `smhub-broker`) | connects in 140 ms; finds the six entities by name (API 1.14): the light ring with 7 effects, Power LED, Blue LED, User Button, Factory Reset Button, the `play_rtttl` service; a second client at the same time disconnects neither |
| `hardware_test.py --sound` | settings merge, refusals and put-back; tokens (read, 403 on token management, 401 for a wrong or revoked one); the ring's colour, Pulse and Rainbow reported back, a brightness change while off turns it on, the look put back exactly; the blue LED; an infrared code saved, sent, renamed, exported and deleted, raw timings sent; a beep played ("Playing song Beep" in ESPHome's log) and a repeat dropped |
| The ring seen by a person | the colour order and single LEDs right; the seven effects (Pulse, Strobe, Scan, Color Wipe, Random, Fireworks, Rainbow) as described; the effect back after a restart of the daemon |
| The ring driven by the hub itself (the Light ring switch off on the ESPHome page, the hub restarted) | `/dev/spidev0.0` present, the light's owner `linux`, the other devices still `esphome`; the hub reports the ring's real state |
| The Light ring switch on again, the hub restarted | no `/dev/spidev0.0`, every owner `esphome`, connected; `hardware_test.py` passes |
| Infrared learning, a TV remote using RC-6 | learned as `rc6_0`, code `0x58`, 39 pulses; afterwards the decoders are off and no helper is left. Learning waits past captures of fewer than 8 pulses; no helper is left after a cancel or after the 30 s run out; "Listen for remotes" ends its helper when switched off |
| BTN1 and BTN2, pressed by a person | a short press is "short" 0.38–0.46 s after the release; two presses 0.33–0.39 s apart are "double"; a hold of 1.4–1.7 s is "long" at the release, and the page shows the real hold time; BTN2 held 6.5 s does nothing in the hub, and `smhub-reset-daemon` sees each BTN2 press and acts only on its 10 s hold |
| A button action, short press set to "switch the light ring" | the first press turns the ring on 0.39 s after the release, the second turns it off |
| Pages served by the hub (Buzzer, Lights, Infrared remote, Buttons, Accounts with tokens, System with Find my hub, the home page's Remote card) | live data, no console errors; `pages.spec.ts` for them and for ESPHome and the home page, both levels, phone and desktop: 28 of 28 |

### The UPS board

The UPS board page is in development builds only (`VITE_UPS=1` at build time
puts it in, `ui/src/shell/routes.ts`); the `/ups` API is in every build.

| Check | Result |
|---|---|
| Marking and unmarking the board through the API, nothing wired | not marked by default; marking it: `features.ups` true, `ups-i2c2` in `uEnv.txt` at once (the OS's `dtbo-watcher`), a scan before the bus is on 409; unmarking takes the overlay out of `uEnv.txt` and `features.ups` back to false |
| The UPS overlay, offline | `ups-i2c2.dtbo` applied with `fdtoverlay` to the hub's own device tree together with its ESPHome overlays: I2C2 `okay`, alias `i2c2` set, I2C4 still disabled |

### Sign-in and the factory account

Sign-in on and off (D41), checked on the hub by comparing 12-character
fingerprints of the stored hash, never a password: for another user name, for
the factory name and for a third name, sign-in on changes the factory hash
and keeps the old one aside, and the open API answers 401; sign-in off gives
the factory hash back and keeps nothing aside. 26 of 26; afterwards the same
accounts, security settings and factory hash as before.

### SD card

On the development hub (a model with a slot, no card), `features` reports
`"sdcard": false, "sd_slot": true`; the home page's storage card says "Not
inserted" and General says "No SD card inserted.".

### Measurements

The daemon, on the hub:

| What | Result | Build and method |
|---|---|---|
| Memory | RSS 6.8 MiB, 1.2 MiB of it the daemon's own (anonymous); 2 threads | release packages |
| Memory, idle | RSS 5.2 MiB, 1.2 MiB anonymous | builder-image build without the hardware modules, `measure.sh`; the hub's kernel has no `smaps`, so private memory is read as anonymous RSS |
| Memory, one browser connected | RSS 5.4 MiB, 1.5 MiB anonymous; peak RSS 6.6 MiB | the same |
| Memory with the hardware modules, ESPHome connected | RSS 5.7–5.9 MB, 1.1–1.2 MB its own, 2 threads | builder-image build |
| Threads | 2, idle and busy | every build |
| CPU, idle, no browser | 1 tick of 10 ms in 600 s: 0.002 %; with the hardware modules 0 ticks in 60 s | builder-image builds |
| CPU while the hub drives the ring | an effect 0.7–0.8 % of a core (25 frames a second); a steady colour is written once, then 0 ticks in 20 s | builder-image build, `/proc` over 20 s |
| Ready after the process starts | 325–359 ms; about 4 s into boot | builder-image build |
| A simple GET on the hub | 3.5 ms straight to smhubd, 5.6 ms through nginx (median of 20) | `measure.sh` |
| One full read of service state | 25 ms from `/run/openrc` (`rc-status -a` takes 0.31 s of the core on an idle hub) | builder-image build |
| The whole system after boot | 103 MB used of 488 MB | builder-image build without the hardware modules |
| Stripped binary | 6,218,008 bytes, static (builder-image build of 2.0.3-1). Of it, the hardware modules: encryption for ESPHome's API (`snow`: X25519, ChaCha20-Poly1305, SHA-256) 71 KB, `smhub_api::hw` 101 KB, the ESPHome client 10 KB, its tables 10 KB, the device code 5 KB, RTTTL 4 KB, tokens 4 KB; the rest is generic code under `tokio`, `serde` and `axum` | builder-image build, by symbol name in an unstripped build |
| Installed | 24 MB with the radio flasher | builder-image build without the hardware modules |

The web UI:

| What | Result | Method |
|---|---|---|
| First load, uncompressed as nginx sends it | 273 KB in 4 files (budget 400 KB) | `npm run build` size report |
| Requests before the home page is usable | 6 and one WebSocket, 231 KB | `load-timing.mjs`, empty cache |
| Requests while a page stays open 60 s | none (live data comes over the one WebSocket) | `load-timing.mjs` |
| Home page usable, empty browser cache | 0.22–0.40 s over the local network, 0.43–0.63 s over Tailscale | `load-timing.mjs`, 3 and 4 runs |
| Home page usable, warm cache | 0.30–0.50 s over the local network, 0.34–0.63 s over Tailscale | the same |
| First visit right after smhubd starts | 3.0 s: every cache on the hub is cold | the same |
| JavaScript heap | 2.1 MB, the same in English, Ukrainian, German, Hebrew and Chinese | Chromium |
| A language other than English | one download of its own file, loaded when chosen: 66–121 KB as built (Ukrainian 100 KB, Thai 121 KB, Chinese 66 KB) | `npm run build` |
| UI files on flash | 3.2 MB with all 31 languages; 0.97 MB with English and Ukrainian | `npm run build`, release build (no UPS page) |
| Accessibility | axe finds no WCAG 2.2 A or AA problem on any page, phone and desktop | `pages.spec.ts` |

"Usable" is `load-timing.mjs`'s point: the overall status and the apps card
show real content.

### 1.x and 2.0 side by side

The same hub runs 2.0, then 1.x (`smhub-services` 1.1.0, `smhub-ui` 1.0.6),
then 2.0 again (a builder-image build without the hardware modules). Each is
restarted, timed from the local network, and measured five minutes after its
kernel starts, with the same scripts (`boot_timing.sh`, `compare_run.sh`,
`compare.sh`, `compare-load.mjs`). The page with charts in English, Ukrainian
and Chinese is [report/index.html](report/index.html).

| What | 1.x | 2.0 |
|---|---|---|
| Kernel start to the page answering with data | 93.2 s | 43.8 s |
| Restart command to the page answering with data | 122.9 s | 73.2 s |
| Backend private memory / RSS / threads, 5 min after boot | 76.6 MiB / 95.4 MiB / 8 | 1.4 MiB / 5.1 MiB / 2 |
| Backend CPU, idle 300 s, no browser | 1.44 % | 0.003 % |
| Page ready, empty cache, local network, median of 5 | 3.35 s | 0.17 s |
| The same, browser CPU slowed 4× | 3.93 s | 0.45 s |
| First visit after the restart | 4.36 s | 2.91 s (0.16 s in a second try) |
| Requests / downloaded / other hosts | 34 / 2.7 MB / Google Fonts | 6 / 232 KB / none |
| Browser script time / JS heap | 331 ms / 11.4 MB | 23 ms / 4.4 MB |
| Hub CPU for 5 visits and 60 s open: backend / whole hub | 13.9 s / 18.1 s | 2.3 s / 6.3 s |
| Routes (median of 20): health, version or system, network, metrics, updates, radios | 9.3, 10.5, 835, 17.7, 565, 14.2 ms | 3.3, 4.9, 247, 5.0, 32.4, 123 ms |
| Installed backend / UI; packages to download | 153.4 / 2.7 MiB; 35.4 MiB | 26.7 / 0.9 MiB; 7.4 MiB |

Radios is the one slower route in 2.0: its answer also names the app that
holds each radio, found by scanning open files. `compare.sh` counts the
backend's own CPU and that of the commands it runs separately.

## Coverage against 1.x

Every page of the 1.x UI (`frontend/src`) is compared with the 2.0 UI and API
by reading the code on both sides.

Not counted, because 1.x users never reach them or they do nothing: the
design mock-ups under `frontend/archive/**` and `Settings/UnderDev.jsx` (no
route reaches them; they read and write a mock JSON document, not the hub),
which include 1.x's infrared, audio, buzzer, button, service LED and SD card
pages; Settings entries marked `status: "dev"` (its menu filters them out);
the social links (they point to `#`); the header's module, battery and network
popovers (never drawn); Settings → Apps (no data, no save).

### Pages

| Area | 1.x | 2.0 |
|---|---|---|
| Sign-in | sign in, sign out, a hub without a password | the sign-in page; Settings → Accounts |
| Header and sidebar | update dot and popover, OS update progress, restart after an OS update | navigation badges, the hub's notices, Updates |
| | theme and language switches | the theme button beside the account; Appearance; Regional settings |
| | the hub's clock on every page | Home and Settings → System |
| | pages, embedded apps, Console, Files | sidebar (Terminal and Files at the advanced level) |
| | restart, hardware revision, OS version | Settings → System |
| Factory backup | prompt, restore, download, delete | the prompt in the first-run setup; Settings → Backups |
| Dashboard | show, hide, reorder cards | Home, arrange; the layout is saved on the hub |
| | service status | Home's apps card; Tools → Services (advanced) |
| | CPU, memory, uptime, load | Home (load at the advanced level) |
| | storage, SD card | Home |
| | network interfaces (IPv4, IPv6, MAC) | Home, Network (MAC at the advanced level) |
| | USB mode and devices | Peripherals → USB (advanced); no home card |
| | model, host name, radios | Home |
| | add-on modules fitted | partly: see [What 2.0 does not do](#what-20-does-not-do-that-1x-does) |
| Settings | sign-in on and off, change a password | Settings → Accounts |
| | host name, time zone, advanced mode, SD card config | Settings → General; the level is per user |
| | backups: include home, extra folders, create, restore | Settings → Backups (extra folders advanced) |
| | MQTT: users, port, other devices, anonymous, bridge, log | Settings → MQTT (port, bridge and log advanced) |
| | WireGuard: status, connect, every field | Network → VPN (keepalive and preshared key advanced; the endpoint is one host:port field) |
| | Wi-Fi: status, scan, connect (hidden, security, static IP), disconnect, forget | Network → Wi-Fi (static IP advanced) |
| | Ethernet: status, addresses, DNS, MTU, autoconnect | Network → Ethernet |
| | LTE: presence, registration, signal, APN, start on boot, LED, power, reset | Network → LTE (reset advanced) |
| | ESPHome: switches, Bluetooth proxy, view and make a key, restart | Settings → ESPHome |
| | serial bridge | Settings → Serial bridge (advanced) |
| | updates: channel, automatic updates, OS versions, install, restart | Updates (older versions advanced) |
| | radios: type, version, mode or region, firmware list, flash, IEEE address | Radios (port and IEEE address advanced) |
| Apps | store: search, sections, install, upgrade, check, upgrade all | Apps |
| | an app: remove, start with the hub, watchdog, sidebar, links, start, stop, restart | the app's page |
| | settings form with read-only fields locked, reset settings, changelog, logs | the app's page |
| | Tailscale and OpenThread panels | the app's page |
| | an app inside the hub: wait, status, logs, start | `/open/<app>` |
| Tools | web terminal | Tools → Terminal (advanced) |
| | files: browse, upload, new folder, rename, move, delete, download, edit, view | Tools → Files (advanced) |

### Hardware

| Hardware | 1.x | 2.0 |
|---|---|---|
| Infrared receiver and transmitter | no page | Peripherals → Infrared remote: learn, send, rename, export, listen for remotes; `/ir` for Home Assistant and scripts (with a token) |
| 3.5 mm audio | no page | nothing: the OS has no driver for it |
| Buzzer | a switch on the ESPHome page (it gives PWM3 to the second core) | the same switch; Peripherals → Buzzer: event sounds, quiet hours, mute, own melodies |
| Light ring | a switch on the ESPHome page (SPI0) | the same switch; Peripherals → Lights: colour, brightness, effects, hub events, night mode |
| User I2C, ADC1, ADC2 | the I2C switch on the ESPHome page; ADC overlays kept from settings, no switch | the same |
| Service LEDs | no page; ESPHome drives them | Peripherals → Lights, through ESPHome; all off, night mode |
| Buttons | no page; holding BTN2 for 10 s resets the hub (the OS's reset program) | Peripherals → Buttons: actions on a short, double or long press; the 10 s factory reset is unchanged |
| SD card | usage on the dashboard, a Files root, "SD card config" on General | the same three; a model with a slot says when no card is inserted |
| 4G modem | power on, reset, LED switch, status | the same, on Network → LTE |
| UPS board | in the dashboard's module list (read from a line) | Peripherals → UPS, marked by the user (D57), in development builds only; not in the release |

### Callers of API v1

2.0 answers two API v1 addresses: `GET /api/v1/login/user-session`, which the
OS image's readiness check asks, and `POST /api/v1/login/cookie`, which a
cached 1.x UI calls to sign in (D51). Every other `/api/v1/` route is gone,
including the unauthenticated `/api/v1/gpio/*`. Among SMLIGHT's software with
source, only the readiness check calls API v1. `smhub-broker`'s source and
scripts on users' own machines cannot be searched
([plan.md](plan.md#open-questions), question 2).

## What 2.0 does not do that 1.x does

Checked against `ui/src`, `api/openapi.yaml` and `crates/`.

1. **No list of fitted add-on modules.** 1.x's dashboard lists 4G, Z-Wave,
   PoE and UPS (`pages/Dashboard/SystemInfoCard.jsx`). In 2.0 a Z-Wave module
   shows as a radio (Radios and Home's radios card), a 4G module as the LTE
   page, and a UPS board is marked by the user in development builds only. A
   **PoE module is shown
   nowhere**: the daemon detects it and the API reports `features.poe`, but
   the UI does not read it.
2. **No "Z-Wave module installed" on Z-Wave JS UI's page** (1.x:
   `pages/Packages/AppDetail.jsx`).
3. **Embedded apps do not follow the hub's theme.** 1.x posts
   `smhub:setLocalStorage` to the Zigbee2MQTT and Z-Wave JS UI frames so they
   switch between light and dark with the hub (`pages/AppFrame.jsx`); 2.0
   sends nothing to an app's frame.
4. **Fewer Wi-Fi details.** 1.x shows the channel, the signal in dBm, the
   maximum bit rate and the BSSID column of a scan (`pages/Settings/Wifi.jsx`).
   2.0 shows signal bars and the band; the API has no fields for the rest.
5. **The ESPHome key cannot be typed or pasted.** 2.0 makes a random key;
   1.x also takes one typed in, for example to match an existing Home
   Assistant entry (`components/Settings/EncryptionKeyInput.jsx`). The API
   accepts any valid key.
6. **The ESPHome page's log link is not filtered** to the second core's lines
   (advanced level). The link passes `filter=smhub-rtos`, but the Logs page
   reads only `name`.
7. **No list of which apps' data a backup will hold before it is made**
   (1.x: `pages/Settings/Backups.jsx`). 2.0 lists the apps only for a backup
   file that is chosen for restore.
8. **The factory backup's file name and apps are not shown**
   (1.x: `components/FactoryBackupModal.jsx`). The API's `FactoryBackup` has
   the date and size only.
9. **An installed community app is not marked.** 1.x tags it and shows its
   disclaimer on the app's page (`pages/Packages/AppCard.jsx`,
   `AppDetail.jsx`). In 2.0 the notice is on the store's Community tab only,
   and an installed app's section shows at the advanced level only.
10. **Files: no selecting several at once** for delete, download or move
    (advanced level; 1.x: `pages/FileManager.jsx`).

## Known issues

### In 2.0

- The first visit right after smhubd starts takes 3.0 s, over the 1 s target
  in [vision.md](vision.md#targets): every cache on the hub is cold. Every
  other visit measured meets it.
- Each language file is 66–121 KB as built, over the 40 KB target per
  language.
- The hub cannot read back an infrared code it sends itself: the kernel's
  sender driver (`gpio-ir-tx`) keeps interrupts off while it sends (79 ms for
  one NEC code), so the hub's own receiver sees one late edge.
- Flashing radio 2 (EFR32MG21) fails on the development hub, as it does with
  1.x (see [Radios](#radios)).

### Going back to 1.x

- 1.x's serial bridge page reads the ser2net file 2.0 writes (D58) through a
  YAML reader that refuses repeated keys: with one port it writes its own map
  again, which the hub's ser2net refuses, as it does with 1.x alone; with two
  or more ports its save fails until the ports are removed. ser2net itself
  keeps working from 2.0's file.
- 1.x's MQTT page, saved once, writes `allow_anonymous false` into the OS
  image's broker file, and Zigbee2MQTT as installed then cannot
  connect (D53). This is 1.x's own behaviour.

### In the OS

These come from the OS image and affect hubs with 1.x and 2.0 alike.

- **A powered 4G modem without mobile data takes the hub off the internet.**
  A restart does not cut the built-in modem's (A76XX) power, so it comes up
  as a USB network (`eth1`, RNDIS). NetworkManager gives its default route
  priority 100, ahead of the Ethernet cable's 101, and without mobile data
  that route goes nowhere: the hub has no internet, and Tailscale, which needs
  its control server after a restart, shows "logged out" (its state is
  intact). Powering the modem off with the OS's own sequence (`modem-power`,
  `gpioset -t 100ms,2500ms,0 4g_power=0`) brings the route, the internet and
  Tailscale back within seconds. The modem's network should rank below
  Ethernet and Wi-Fi unless the user chooses LTE. The LTE page's power on and
  reset leave the modem powered.
- **The OS has no audio driver**, so the 3.5 mm output cannot be used.
- **nginx sends the UI files without cache headers**, so a browser can keep an
  outdated `index.html` for days after an update. 2.0 finds its way to the new
  UI by itself (D51).
- **The Nano's `nano-leds` service may drive the wrong lines.** It opens
  `gpiochip2` on OS versions after 0.8.0, while the LED lines are on
  `gpiochip3` on the development hub. This is read from the code; no Nano is
  tested.
- **The status LEDs' state at boot is not always explained.** ESPHome on the
  second core owns `led_pwr` and `led_cus` and restores their saved state at
  each start; nothing else drives them on an Essential or Premium hub. Both
  can come up on at boot where the saved state has one on (seen once on the
  development hub); the cause is not known.

## Not verified yet

What [plan.md](plan.md#before-the-stable-channel) requires before the stable
channel, and what the tests above leave out:

- **Other models:** SMHUB Premium, Nano and Nano MR, and other hardware
  revisions. Everything above is from one SMHUB Essential.
- **Other OS versions** than 1.0.2, and an OS update with 2.0 installed.
- **The oldest supported 1.x version:** upgrade and going back are tested
  from 1.1.0-1 and 1.0.6-1 only.
- **Real setups:** hubs with sign-in on, apps holding data, Wi-Fi, VPN and LTE
  configured; every row of [what must not break](vision.md#what-must-not-break)
  after the update and after going back.
- **Backups across versions:** a 1.x backup fully restored on 2.0 (2.0's
  restore check reads one), and a 2.0 backup restored on 1.x.
- **A factory reset** with 2.0 installed, and restoring a factory backup.
- **Browsers other than Chromium:** Firefox and Safari, on a computer and on
  a phone.
- **A complete click-through on the release packages.** Its complete pass is
  of a build without the Peripherals section and the Regional settings page.
- **Other store apps:** Node-RED, Z-Wave JS UI's full cycle, OpenThread and
  PicoClaw are not run through `clickthrough.mjs`'s every-app step.
- **Radio 3 (Z-Wave):** no firmware is offered for it.
- **The serial bridge's file format (D58) on a hub:** checked with ser2net
  4.3.11 (Debian 12, which refuses 1.x's file as the hub does) and 4.6.4
  (Alpine): one port, three ports and kept entries all open their ports.
- **Night mode and quiet hours by the hub's local time:** checked with smhubd
  in a container at `TZ=IST-5:30`, not on a hub.
- **The UPS board's I2C bus after a restart** with the overlay applied: the
  overlay is checked offline only.
- **The hardware, further:** a learned infrared code sent to the device it
  came from; the LEDs switched by the hub itself (needs a hub without the
  ESPHome firmware); the button's other actions (a sound, an infrared code, a
  call to an address); the buzzer played by the hub itself; the sounds heard
  by a person.
- **A visible response within 100 ms** of a tap or click is not measured on
  its own.
- **Saving an Ethernet change, logging Tailscale out, confirming power off**
  and **installing an OS version** from the UI: left out on purpose (see
  [Pages and actions](#pages-and-actions)).

## Translations

The UI has 31 languages. English is the source text. Ukrainian is the other
language of the 1.x UI. The other 29 are machine translations (D36):
Bulgarian, Chinese (simplified), Croatian, Czech, Danish, Dutch, Estonian,
Finnish, French, German, Swiss German (made from German with "ss" for "ß";
nothing else differs), Greek, Hebrew, Hungarian, Italian, Japanese, Korean,
Latvian, Lithuanian, Norwegian Bokmål, Polish, Portuguese (Portugal),
Romanian, Serbian (Latin), Slovak, Slovenian, Spanish, Swedish and Thai. No
native speaker has read them.

Every file passes `ui/scripts/check-locale.mjs`: 1,413 texts each, the same
keys and placeholders as English, exactly the plural forms the language uses.
Hebrew lays out right to left; values inside a sentence are bidi isolates and
number spans take their own direction, so "1:40 PM" and "175 MB" read
correctly. Which languages need a native review before the stable release is
an open question ([plan.md](plan.md#open-questions), question 4).

### Points to check first

The translators' doubts, across languages:

- **"hub"** is the most frequent word and its choice shapes the whole file
  (see the table). Settle it first.
- **"Factory backup"** (`backups.factory`) reads literally as "a backup made
  at the factory", while it is the copy kept from before the last factory
  reset. Most translators wrote "backup from before the reset". Saying that in
  English too, then aligning the others, would remove the doubt.
- **"Radios"** is "radio modules" or similar in most languages, because the
  plain word reads as a broadcast radio.
- **"Core version"** (`sys.core`) reads as the Linux kernel in several
  languages; the label may need other wording.
- **MQTT terms** (broker, topics, clean session, bridge) are often kept in
  English by the communities; the translators differ.
- **Product names** are kept as they are, including Ethernet, Bluetooth and
  Wi-Fi, even where the OS uses a local word (German "WLAN", Chinese
  以太网/蓝牙).
- **"Light ring"** is the term on the Lights page and on the ESPHome page's
  switch alike.
- **"Revoke"** (a token) needs its own word where "Cancel" or "Undo" already
  take the obvious one (Bulgarian, Chinese, Latvian, Lithuanian, Hebrew).
- **"Learn a button"** reads as the user learning in several languages; most
  translators wrote "memorise" or "read" a button.
- **"Colour wipe", "Chime", "Quiet hours"** and the raw infrared "timings"
  have no settled terms.
- **API "token"**: some languages keep the loanword, others use a local word
  (Estonian "tõend", Finnish "tunnus", Greek "διακριτικό", Hebrew "אסימון",
  Latvian "marķieris", Lithuanian "prieigos raktas").
- **The LEDs** are "LED", "lights" or "indicators" depending on the language;
  French and Dutch say "voyant" / "lampje" for a status LED in some texts.
- Lithuanian shortens the infrared page's name to "IR pultas" for the menu.

### Per language

| Language | File | "hub" | Points to check |
|---|---|---|---|
| Bulgarian | `bg.json` | хъб | formal address; "Версия на ядрото" (core version) may read as the kernel; "feed section" has no standard term |
| Chinese (simplified) | `zh.json` | 网关 (gateway); the IP gateway is 默认网关 | 中枢 is the alternative for hub; MQTT broker → MQTT 服务器; APN example `cmnet` |
| Croatian | `hr.json` | čvorište | "one" forms keep `{count}` (they also cover 21, 31…); firmware → firmver; USB host → domaćin |
| Czech | `cs.json` | hub (declined) | centrála is the alternative; formal "vy"; "Nahrát firmware" also means upload |
| Danish | `da.json` | hubben | "du"; Download/Upload kept as loanwords; "Emner" for MQTT topics |
| Dutch | `nl.json` | hub | "je"; hyphenated compounds ("app-lijst"); "Brug" for bridge |
| Estonian | `et.json` | keskus | MQTT broker → vahendaja; update → uuendus |
| Finnish | `fi.json` | keskitin | broker → välittäjä; upload → Lähetä, download → Lataa |
| French | `fr.json` | hub | "vous", French spacing before : ; ? !; firmware → micrologiciel; upload → Importer |
| German | `de.json` | Hub | "Sie"; radios → Funkmodule; "Wi-Fi" kept although "WLAN" is usual |
| Swiss German | `de-ch.json` | Hub | German with "ss" for "ß"; nothing else differs |
| Greek | `el.json` | κόμβος (also means "node") | κεντρική μονάδα is clearer but long; radios → Πομποδέκτες |
| Hebrew | `he.json` | רכזת | right to left; gender-neutral forms; "port" → יציאה; app names masculine |
| Hungarian | `hu.json` | hub | formal "Ön"; Advanced → Haladó; router wording differs in one place |
| Italian | `it.json` | hub | "tu"; app names feminine, services masculine; "Live" → In diretta |
| Japanese | `ja.json` | ハブ | radios → 無線モジュール; update アップデート vs refresh 更新 |
| Korean | `ko.json` | 허브 | password → 비밀번호; radios → 무선 모듈; firmware "install" wording |
| Latvian | `lv.json` | centrmezgls | firmware → aparātprogrammatūra (long); "one" forms keep `{count}` |
| Lithuanian | `lt.json` | centralė | firmware → mikroprograma; "one" forms keep `{count}` |
| Norwegian Bokmål | `nb.json` | huben | "du"; broker kept; Swap → Vekselminne |
| Polish | `pl.json` | hub (declined) | firmware → oprogramowanie; radios → Moduły radiowe; "feed section" |
| Portuguese (Portugal) | `pt.json` | hub | "a app"; download → Transferir; "Ligar" means both connect and power on |
| Romanian | `ro.json` | hub (hubul) | formal; Cancel and Undo both "Anulați"; MQTT topics → Topicuri |
| Serbian (Latin) | `sr-latn.json` | čvorište | "hab" is the loanword alternative; "one" forms keep `{count}`; Settings → Podešavanja |
| Slovak | `sk.json` | centrála (feminine) | "hub" avoided: its case forms read as "mouth/mushroom" in Slovak; logs → denníky |
| Slovenian | `sl.json` | središče | firmware → programska oprema; "one" forms keep `{count}` except `dash.allApps`; sign in → Prijava |
| Spanish | `es.json` | hub | "tú"; Agregar, Configuración; buzzer → zumbador |
| Swedish | `sv.json` | hubben | "återställ" means both restore and reset; broker kept; topics → Ämnen |
| Thai | `th.json` | ฮับ | radios → โมดูลวิทยุ; logs → ล็อก (บันทึก is Save); no "?" in questions |
