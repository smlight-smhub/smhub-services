# Key decisions

Every stack and architecture decision of `smhub-services` and `smhub-ui` 2.0,
with its reason. Code
and [AGENTS.md](../AGENTS.md) refer to decisions by number. Each one states what
is decided and why, with the priority it serves (numbered as in
[vision.md](vision.md)), and, where it matters, the alternatives it rejects and
how it is checked.

Priorities: 1 stable and mature, 2 RAM, 3 CPU, 4 future-proof, 5 flash.

| Section | Decisions |
|---|---|
| [Scope and migration](#scope-and-migration) | D1–D6 |
| [API](#api) | D7–D10 |
| [Daemon: language, build, dependencies](#daemon-language-build-and-dependencies) | D11–D15 |
| [Daemon: runtime and HTTP](#daemon-runtime-and-http) | D16–D19, D52 |
| [Daemon: data](#daemon-data) | D20–D22 |
| [Daemon: operating system](#daemon-operating-system) | D23–D27, D49, D50, D53, D58 |
| [Daemon: the hub's own hardware](#daemon-the-hubs-own-hardware) | D54, D57 |
| [Web UI: stack](#web-ui-stack) | D28–D34, D51 |
| [Web UI: product](#web-ui-product) | D35–D39 |
| [Security](#security) | D40–D43, D55 |
| [Quality](#quality) | D44–D48 |
| [Direct dependencies](#direct-dependencies) | the crates and npm packages |

## Scope and migration

### D1. Rewrite the backend and the web UI, and redesign the API between them

- **Decision:** `fastapi-backend` and `system-config` (1.x) are replaced by one
  Rust daemon, `frontend` by a new web UI, and API v1 by API v2. Nothing of
  1.x's backend, frontend or API has to be kept. The OS image and the apps are
  not rewritten.
- **Why:** 1.x's backend costs what its Python framework stack costs (32 s of
  CPU to import, 84–108 MiB). Its web UI is 2.5 MiB of JavaScript in one bundle
  and takes 34 requests before a page is ready. Rewriting both at once lets the
  API be designed for what each side needs, instead of copying 1.x's.
- **Rejected:** a lean Python backend (still an estimated 10–15 s and 40–50
  MiB); keeping API v1 so the 1.x UI could stay; a web UI written in Rust and
  compiled to WebAssembly (a larger download and a young ecosystem).

### D2. One repository for the daemon, the web UI and the API contract

- **Decision:** this repository holds the Rust workspace, the web UI and the
  API description. It builds both packages, `smhub-services` and `smhub-ui`.
- **Why:** a change to the API touches the contract, the daemon and the UI in
  one commit, so they cannot drift apart (1). In 1.x the backend pins the
  frontend as a submodule, and the pin points at an unmerged branch.
- **Rejected:** separate repositories with the contract in a third place.

### D3. The user's setup is the only compatibility contract

- **Decision:** the update to 2.0 must leave an existing hub working as its
  owner set it up. Concretely:
  - the database keeps its file, tables and data (D21);
  - config files of the OS and of apps are written so that the same setting
    gives the same file as in 1.x (one exception: the serial bridge's file,
    D58);
  - app packages and their `schema.json` work unchanged;
  - app and OS updates, channels, the OTA hook, backups and factory reset
    behave as in 1.x, and a backup from 1.x restores on 2.0;
  - the 1.x packages can be installed again and work.

  The full list is in [vision.md](vision.md#what-must-not-break).
- **Why:** this is the one hard requirement. It protects what users own and
  leaves every technical choice free (1, 4).
- **Check:** upgrade and downgrade tests (D46). They pass on the development
  hub; hubs on the stable channel keep 1.x until they also pass on other hubs
  and models (D5).

### D4. Keep package names and paths

- **Decision:** packages `smhub-services` and `smhub-ui`, OpenRC service
  `smhub-services`, `/opt/bin/smhub-services`, port 8000, socket
  `/run/smhub-backend.sock`, database `/opt/smhub-services/config/backend.db`,
  UI files under `/opt/smhub-ui`, log file `/var/log/smhub-services.log`.
- **Why:** 2.0 then reaches a hub as an ordinary package update, and the OS
  scripts, the OTA hook and nginx keep working without change (1). This is what
  "update behaviour stays the same" means in practice.
- **Result:** the OS image is not changed. What it expects from the backend is
  listed in [architecture.md](architecture.md#what-the-os-image-expects).

### D5. Replace in one release, proven first on the development channel

- **Decision:** 2.0 replaces 1.x in one release, as the next major version of
  the two packages. Hubs that opt in get it first: from the development channel
  of SMLIGHT's package feed (built by SMLIGHT's package builder from a commit of
  this repository) and from GitHub pre-releases, installed with
  `packaging/update-beta.sh`. Hubs on the stable channel keep 1.x until 2.0 has
  passed upgrade tests on hubs with real setups other than the development hub,
  on the other models, from the oldest supported 1.x version, and in browsers
  other than Chromium ([plan.md](plan.md)). 1.x is not run beside 2.0. Going
  back means installing the 1.x packages, which works because the data is
  unchanged (D3).
- **Why:** no code is written only to keep 1.x alive during a transition, and
  the update channels the hubs already have are used as they are (1).
- **Rejected:** running the Rust daemon in front of the Python backend and
  forwarding API v1 to it. It gives an earlier boot improvement, but needs
  forwarding, two sign-in systems and two backends in memory, all of which
  would be thrown away.
- **Cost:** hubs on the stable channel get the gain only when 2.0 reaches that
  channel.

### D6. Radio flashing stays in Python, on demand

- **Decision:** a small Python helper (`helpers/radio-flash/`) wraps
  `universal-silabs-flasher` and `smlight-cc-flasher`, the same library files
  the 1.x package ships (`packaging/vendor/`). The daemon starts it for one
  flash, reads its progress, and it exits. Before a flash the daemon stops the
  app that holds the radio's port and starts it again afterwards (D25).
- **Why:** those libraries are proven on these radios, and a mistake can brick a
  radio (1). A helper that runs only during a flash costs nothing at boot or
  idle (2, 3). The helper and the libraries are GPL-3.0 and run as a separate
  program, apart from the daemon.
- **Rejected:** porting the flashing protocols to Rust.

## API

### D7. One contract file is the source of truth

- **Decision:** API v2 is described in one OpenAPI file, `api/openapi.yaml`.
  The UI's TypeScript types are generated from it (`npm run gen:api`). A change
  to the API starts with a change to this file; the daemon follows it.
- **Why:** both sides are built from the same description, so they cannot
  disagree silently (1, 4). Nothing is added to the daemon at run time.
- **Not in place:** an automated check of the daemon's answers against the file
  (the contract tests of D46). The UI's side is checked by its compiler.
- **Rejected:** generating the description from the daemon's code (a proc-macro
  dependency in every route); hand-written types in the UI (unchecked casts, as
  in the SLZB-OS UI).
- **Open:** whether anything besides the 1.x UI calls API v1 (Home Assistant,
  scripts, `smhub-broker`, other tools). 2.0 answers only the two API v1
  addresses that the OS image's readiness check and a cached 1.x UI use (D48,
  D51).

### D8. Resources, with one set of conventions

- **Decision:**
  - `GET /api/v2/<resource>` returns its state and settings.
  - `PATCH` changes settings. The whole change is validated first; on an error
    nothing is applied and the answer names the field.
  - `POST /api/v2/<resource>/<action>` runs a command. No GET changes anything.
    A command that takes time answers with an operation, whose progress arrives
    on the event connection (D9).
  - Errors have one shape with a stable code; the UI translates the code.
  - Values are in canonical form: UTC timestamps, sizes in bytes, durations in
    seconds, temperatures in degrees Celsius, currency as an amount with its
    code. Formatting is the UI's job (D37).
  - Secrets are write-only: a GET says whether one is set, never its value.
  - A path segment carries only identifiers the hub makes (package and service
    names, ids, log names). A value from outside, such as a Wi-Fi name, an MQTT
    user name or an SSH key fingerprint, is a query parameter: the hub's nginx
    decodes `%2F` in the path before it passes a request on, which would split
    such a value.
- **Why:** one pattern for every page keeps both sides small and predictable
  (1, 4).

### D9. One event connection that starts with the current state

- **Decision:** the UI opens one WebSocket and subscribes to the resources the
  page shows. The daemon answers each subscription with the resource's current
  state and then sends changes. Long operations (installing an app, an OS
  update, a flash, a backup) report progress on the same connection. Byte
  streams have their own endpoints: the terminal, log following, file transfer
  and backups. Uploads are sent in pieces of at most 768 KiB, under the 1 MiB
  request limit of the hub's nginx, so they need no OS change and an
  interrupted upload can continue.
- **Why:** a page needs one connection instead of dozens of requests, and
  nothing is polled (3). The SLZB-OS UI subscribes the same way but still
  fetches each resource separately: 22 requests on its dashboard.
- **Measured** on the development hub ([README](../README.md#measured)): the
  home page is ready after 6 requests and the one WebSocket, against 34 requests
  for 1.x; a page left open for 60 s makes no HTTP request.

### D10. The hub describes itself

- **Decision:** one `system` resource states the model, what hardware and
  features this hub has, versions, and whether sign-in is required. The UI shows
  or hides pages from it. It comes from the hardware table (D27).
- **Why:** the 1.x UI keeps its own model list and reads GPIO lines to detect
  add-on modules. With `system`, a new model or module needs no UI change (4).

## Daemon: language, build and dependencies

### D11. Rust on the stable toolchain, pinned

- **Decision:** one exact stable Rust version, 1.95.0, in `rust-toolchain.toml`,
  which the builder image (`packaging/Dockerfile.build`) uses too. Edition 2024,
  no nightly features; the workspace's minimum (`rust-version`) is 1.95. The
  version is raised on purpose, in its own change. SMLIGHT's package builder
  compiles with its own Buildroot Rust toolchain, which must meet that minimum
  (D12).
- **Why:** reproducible builds and no surprises from the compiler (1).

### D12. One statically linked binary

- **Decision:** the repository's build (`packaging/build-ipk.sh`, which makes
  the GitHub pre-releases) is one statically linked binary for
  `riscv64gc-unknown-linux-musl`, a Tier 2 Rust target. The C compiler for the
  bundled SQLite is `zig` 0.13.0 through `cargo-zigbuild` 0.19.8, both pinned in
  the builder image. SMLIGHT's package builder builds the feed's packages from
  the same workspace with its Buildroot toolchain, linked against the image's
  glibc. The code builds both ways, and both builds pass the API and hardware
  tests in `tests/hub/` on the development hub.
- **Why:**
  - A static binary does not depend on the libraries in the OS image, so an OS
    update or a rollback to the other slot cannot break it (1). In 1.x a Python
    version change forces the old slot to be marked bad.
  - `zeesm` measured 28–35 % less private memory with musl than with glibc (2),
    at the price of a little more CPU (3). On the development hub the static
    build of smhubd runs at 5.1 MiB resident and the glibc build at 6.8 MiB;
    their own memory is about the same (1.2–1.4 MiB).
  - `zeesm` ships the same build on the hub. `zig` is pre-1.0, but it is only
    a build tool, pinned to one version.
- **Size:** the stripped static binary is about 6.2 MB
  ([Direct dependencies](#direct-dependencies)).

### D13. Dependency policy, for Rust crates and npm packages

- **Decision:** something becomes a direct dependency only if all of these
  hold: it is widely used, maintained, released as stable (no pre-release), and
  it replaces a meaningful amount of code we would otherwise write and test.
  Its licence is permissive or MPL-2.0 ([AGENTS.md](../AGENTS.md#license)).
  Lock files are committed. Each direct dependency has a row in the tables at
  the end of this page.
- **Why:** priority 1. `zeesm` has 467 crates in its lock file; the daemon has
  149 for the hub target, against an aim of under 200. The 1.x UI commits a
  400 MB package cache; the 2.0 UI has eight runtime packages, against an aim
  of fewer than ten.
- **Not in place:** an automatic check of advisories and licences. The
  repository has no continuous integration.
- **Note:** several standard Rust crates carry 0.x version numbers (`axum`,
  `rusqlite`, `nix`). Maturity is judged by adoption and maintenance, not by the
  number.

### D14. Release profile: size first

- **Decision:** `opt-level = "s"`, fat link-time optimisation, one codegen
  unit, symbols stripped, panics unwind (D44). SMLIGHT's package builder sets
  the same profile for its build.
- **Why:** smaller code means fewer program pages in memory (2) and less flash
  (5).

### D15. A workspace of small crates

- **Decision:** five library crates and one binary, with a fixed direction of
  dependencies; see [architecture.md](architecture.md#daemon-workspace).
- **Why:** `zeesm` put everything in one crate and reached a 35.6 MB binary and
  a compiler that needed over 3.5 GiB of memory. Small crates build faster,
  keep layers honest, and let a part be replaced (1, 4).

## Daemon: runtime and HTTP

### D16. Tokio, on a single-threaded runtime

- **Decision:** Tokio with the current-thread runtime, plus a blocking pool
  capped at 4 threads for file work and password checks. All code stays `Send`,
  so switching to the multi-thread runtime is one line.
- **Why:** Tokio is the standard async runtime (1). The hub has one core, so
  more worker threads add memory and locking, not speed (2, 3). Keeping code
  `Send` leaves the door open for multi-core hardware (4). Measured on the
  development hub: 2 threads, idle and busy.
- **Rejected:** a multi-thread runtime; other runtimes; a thread per
  connection.

### D17. Everything bounded, nothing polls

- **Decision:** every channel, buffer, cache, connection count, upload and
  session count has a limit written in the code and in
  [architecture.md](architecture.md#limits). File changes are watched with
  inotify; network changes come from one `nmcli monitor` process (D24); sockets
  are event-driven; a terminal is read when the kernel says there is data. Work
  that repeats, such as the live metrics every 3 s, runs only while a page
  shows it.
- **Why:** bounded memory (2), no wake-ups while idle (3), predictable behaviour
  under load (1).

### D18. axum on hyper; nginx and stunnel stay in front

- **Decision:** `axum` with HTTP/1.1 only. The daemon listens on
  `127.0.0.1:8000` and on the local socket, nowhere else. nginx keeps serving
  the UI files and proxying to the daemon; stunnel keeps terminating TLS.
- **Why:** nginx and stunnel are mature, already on the hub, and small (about
  1 MiB private each, measured) (1, 2). No TLS library and no static file
  server in the daemon (2, 5). Binding to loopback closes the direct access to
  port 8000 from the LAN that 1.x leaves open.
- **Rejected:** serving static files and TLS from the daemon. The per-app TLS
  proxies need stunnel anyway.
- **OS image:** unchanged. The daemon keeps the port and the address prefixes
  that the image's nginx forwards.

### D19. No HTTP client library in the daemon

- **Decision:** the daemon makes its few HTTP requests with the system `curl`:
  to services on the hub itself (the OpenThread border router's REST API), to
  the internet (update manifests, firmware files), and to the address a button
  action names (D54).
- **Why:** `curl` is as mature as software gets and uses the OS's certificates
  (1). The daemon carries no HTTP client, no TLS stack and no certificate store
  (2, 5). These requests are rare, and the update check is cached for an hour.
- **Rejected:** `reqwest` with `rustls`. It can be added behind the same
  interface if a need appears.

### D52. A change runs to its end, whatever the browser does

- **Decision:** every request that changes something (anything but `GET` and
  `HEAD`) runs in its own task, so closing the tab or navigating away does not
  stop it half-way. Commands the daemon starts are still stopped when they run
  past their time limit.
- **Why:** a request is dropped when the browser goes away, and with it the
  command it runs. A Wi-Fi join cut off when the page changes leaves the profile
  it created behind, and an `rc-service … restart` or a network change could
  stop in the middle.

## Daemon: data

### D20. SQLite stays, through rusqlite, with plain SQL

- **Decision:** the same database file. `rusqlite` with SQLite compiled in. One
  connection per database file (D21), both owned by one thread and fed by a
  bounded queue. SQL statements are written by hand; there is no ORM.
- **Why:** SQLite is the most deployed database there is and right for one
  device (1). One connection per file and small page caches (256 KiB for
  `backend.db`, 64 KiB for `smhubd.db`) keep memory flat (2). The database is
  small (the seed file is 80 KB).
- **Rejected:** an ORM or an async SQL layer; another storage engine (the
  existing data would need converting).

### D21. The existing database keeps its structure

- **Decision:** the daemon reads and writes the existing tables of
  `backend.db` exactly as they are, with the same meaning for every column and
  stored value. It runs no migration on them. Anything new it must store
  (sessions, new per-user preferences, API tokens, infrared codes) goes into a
  second small database file next to the first, `smhubd.db`, which 1.x never
  reads. While hubs can go back to 1.x, the existing tables change only
  together with 1.x, in the same release; after that, schema changes are the
  daemon's: forward-only, additive, tracked in SQLite's `user_version`, and
  only when a feature needs one. A hub whose database is older than the Python
  backend's last revision is first brought to that revision by the same steps,
  shipped as SQL and run in one transaction.
- **Why:** it is part of the compatibility contract (D3), and it is what makes
  going back to 1.x safe (1).

### D22. Durability first

- **Decision:** `synchronous = FULL` on both files. The daemon does not change
  the journal mode of the existing file.
- **Why:** hubs lose power without warning (1). Writes are rare, so their cost
  does not matter (3).

## Daemon: operating system

### D23. One crate owns all OS access

- **Decision:** `smhub-sys` is the only crate that reads or writes OS files,
  runs commands, or talks to devices. The API layer sees narrow interfaces.
  Every path is resolved under a root given at start (`--root`).
- **Why:** an OS change touches one place (4). The real daemon can run against
  a prepared directory on a development PC, which is also what the UI is
  developed and tested against (D46).

### D24. NetworkManager and RAUC through their own command line tools

- **Decision:** NetworkManager is reached with `nmcli` in terse mode and RAUC
  with `rauc`, behind one interface in `smhub-sys`. Changes are followed with
  one `nmcli monitor` process that runs only while a browser shows a network
  page. Connections NetworkManager cannot express through `nmcli` options
  (WireGuard) are written as keyfiles in `/etc/NetworkManager/system-connections`
  and loaded with `nmcli connection load`.
- **Why:** `nmcli` and `rauc` are part of the OS image, are what the OS's own
  scripts use, and their terse output is a documented interface (1). A D-Bus
  library (`zbus`) would be the largest dependency (1) and would hold a D-Bus
  connection and its buffers for the daemon's whole life (2), for calls that
  happen when a user opens a network page. Measured on the development hub: the
  network page reads in about 0.5 s, the Wi-Fi list in about 1 s, and nothing
  runs while no one looks.
- **Rejected:** `zbus`; polling.

### D25. Devices through the kernel's own interfaces

- **Decision:**
  - GPIO: the kernel's character device through the libgpiod v2 tools in the
    OS image (`gpioget`, `gpioset`, `gpiomon`), lines named as the OS's own
    scripts name them; no GPIO crate. A line is held only for the moment it is
    read or pulsed, so the OS scripts and the RTOS bridge that use the same
    lines are not blocked. Lines that only the daemon uses are the exception:
    the service LEDs while Linux drives them (D54).
  - Buttons: the kernel's `gpio-keys` key events (D54).
  - EEPROM: the kernel driver's file, read-only.
  - Serial ports: one program at a time. Before a radio is flashed, the service
    whose process holds its port (found through `/proc/<pid>/fd`) is stopped,
    and it is started again afterwards. The modem's AT port is opened only for
    one short exchange.
  - Peripherals owned by the RTOS core: only through the RTOS.
- **Why:** these are the stable kernel interfaces the Python libraries already
  wrap (1). One owner per device prevents the corruption that two readers of a
  serial port cause.

### D26. Services and packages: read files, spawn tools sparingly

- **Decision:** service and package state is read from OpenRC's and opkg's own
  files where their format is stable; `rc-service` and `opkg` are run for
  actions. Service state is read from `/run/openrc` the way OpenRC 0.63's
  `rc-status` works it out (its state folders, and for a started service the
  daemon records and pidfiles that make it "crashed"), without running it. The
  reading is cached for two seconds, and an inotify watch on the state folders
  drops the cache when they change. Changes are pushed to the UI as events.
- **Why:** 1.x spawns `rc-status` for every status request, and its UI can ask
  every 3 s (3). On the development hub `rc-status -a` takes 0.31 s of the
  single core (0.65 s when the core is busy); the daemon's read takes 25 ms.
- **Check:** `smhubd --services` prints the reading;
  `tests/hub/services_compare.sh` and `tests/hub/services_states.sh` check it
  against `rc-status` for every service and every state. An OS release with
  another OpenRC version runs both before it ships.

### D27. Hardware models in one table

- **Decision:** one table describes each model: its radios, ports, GPIO line
  names and features, among them whether the board has an SD card slot
  (`sd_slot`), so a model with a slot says "No SD card inserted" instead of
  leaving the SD card out. The daemon uses it, and the UI receives it through
  the `system` resource (D10). SMHUB and Nano have the slot; Nano MR has none,
  although every device tree enables the SD controller.
- **Why:** 1.x spreads the model list over several backend files and the
  frontend (4). A new model becomes one row.

### D49. The ways in to the hub stay up

- **Decision:** NetworkManager, sshd, nginx and stunnel cannot be stopped
  through the API (restart is allowed), the daemon's own service takes no
  action through the API, and a backup leaves these services and Tailscale
  running. Tailscale can be stopped from its own app page.
- **Why:** 1.x lets any service be stopped and stops every app, Tailscale
  included, for a backup: a user working over Tailscale or SSH can cut
  themselves off, and a backup downloaded over Tailscale breaks halfway.
  Restoring a backup stops everything it restores, as in 1.x.

### D50. Installed packages are read as opkg writes them

- **Decision:** a package counts as installed when its block in
  `/var/lib/opkg/status` says so, whatever the order of the fields.
- **Why:** 1.x's parser stops at fields it does not expect, so packages whose
  block has `Recommends:` before `Status:` (`smhub-services`, `esphome-bin`)
  are recorded as not installed in `apps.installed`. The daemon writes the
  correct value; 1.x, after going back, writes its own value back at its next
  start, so nothing it depends on changes.

### D53. The broker's settings are read as Mosquitto applies them

- **Decision:** the MQTT settings describe what Mosquitto 2 really does. The OS
  image's file has no `listener` line: Mosquitto then listens on the hub only
  and needs no password, and that is shown as such. A save keeps every setting
  the user does not change: "other devices" off writes
  `listener <port> 127.0.0.1`; a bare `listener <port>` that 1.x writes (it
  listens everywhere) is shown as "other devices: on" and kept bare. The page values 1.x stores keep their form.
  Turning the password requirement on asks first, naming apps such as
  Zigbee2MQTT.
- **Why:** 1.x reads a missing `allow_anonymous` as "password required", so the
  first save of its MQTT page writes `allow_anonymous false`. Zigbee2MQTT, which
  connects without a password as installed, then cannot connect, restarts over
  and over and keeps the CPU at 100 %. 1.x's "allow external" switch changes
  nothing either, because a bare port already listens everywhere. Hubs whose
  file 1.x saved behave exactly as in 1.x until their owner changes a switch.

### D58. The serial bridge's file is written as ser2net reads it

- **Decision:** `/etc/ser2net/ser2net.yaml` holds one top-level
  `connection: &<name>` entry per port, with `accepter: tcp,<port>`,
  `connector: serialdev,<device>,<baud>n81,local` and `kickolduser: true` (a
  client that reconnects takes the port over instead of being locked out). The
  name is the device's file name, made unique. The key repeats, so the daemon
  writes the file as text. It reads both that form and 1.x's `connections:`
  map, and the next save writes the new form; a save keeps a hand-edited file's
  `define`, `default`, `admin` and `led` entries and replaces everything else.
  The page's answer says per port whether something listens on it
  (`/proc/net/tcp`, `tcp6`); after a save it waits up to 2 s for the ports. The
  page warns while the bridge runs with a closed port and links to the log.
  This is an exception to "same files, same formats" in the compatibility table.
- **Why:** the hub's ser2net refuses 1.x's file ("Invalid token at the main
  level: connections") and runs on without a port, and OpenRC still counts it
  as started, so 1.x's page says "Running" while nothing can connect. ser2net
  4.3.11 (Debian 12) gives the same message and opens nothing; 4.6.4 (Alpine)
  reads the map too. The repeated `connection` form opens every port on both,
  checked with one and three ports and with kept entries. The bridge does not
  work from 1.x's file on the hub, so no working setup changes.
- **Going back:** ser2net keeps working from the new file. 1.x's page saves
  through a YAML reader that refuses repeated keys: with one port it writes its
  map again (which the hub's ser2net refuses, as in 1.x), with two or more its
  save fails until the ports are removed, which deletes the file. The ports
  stored on the settings page keep their form either way.

## Daemon: the hub's own hardware

### D54. The hub's own hardware, through whoever owns it

- **Decision:** the buzzer, the light ring, the service LEDs, the buttons and
  infrared each get one module in the daemon with one API (`/hardware`, `/ir`).
  The buzzer, the ring and the LEDs have two backends: ESPHome (the firmware on
  the second core owns the device) and Linux (the daemon drives it). The owner
  is read, not configured: PWM and SPI present in Linux, the ESPHome firmware
  present. A crash of ESPHome never moves a device. The daemon reads ESPHome's
  state and events as a second client of its API (port 6053), with the hub's
  API key; `rtos-notify` is the fallback for commands. The buttons (BTN1, and
  BTN2 with its factory reset) have one backend on every model: the kernel's
  `gpio-keys` key events, which `smhub-broker` and `smhub-reset-daemon` read
  side by side; BTN2's long press ends at 3 s so the 10 s factory reset stays
  as it is. Every caller passes the same queues and limits; every sound and
  light feature is off until the user turns it on. The design and its reasons
  are in [peripherals.md](peripherals.md).
- **Why:** Home Assistant keeps its ESPHome entities, the hub's page shows the
  real state, and the OS's unused C services are not needed (D3, priorities
  1–3). For the buttons, the key events need no second core, work on Nano and
  Nano MR and cost nothing between presses, while `gpiomon` cannot take lines
  the kernel holds ("Device or resource busy" on the development hub)
  (priorities 1–3).

### D57. A UPS board is marked by the owner, and gets its I2C bus by an overlay

- **Decision:** whether an SMLIGHT UPS board is on J10 is a switch the user
  sets (Peripherals → UPS, `PATCH /ups`), not a line the hub reads;
  `features.ups` reports the switch. While it is on, I2C2 on J10's pins 1 and 3
  is on for talking to the board: the
  daemon's package ships the overlay `ups-i2c2.dtbo` (compiled from
  `packaging/smhub-services/overlays/ups-i2c2.dts` in the builder image), which
  the daemon copies to `/mnt/misc/overlays` and removes again, as the ESPHome
  settings do with theirs; it applies from the next start and names the bus
  `i2c2`. The UPS page is in development builds only (the Vite development
  server, or a build with `VITE_UPS=1`); the release packages leave it out, so
  nobody marks a board there. The API is in both.
- **Why:** J10 pin 5 reads "fitted" on a hub without a UPS board (the pad's
  pull-down; Linux cannot set a pull-up), so a read line would be wrong both
  ways. I2C2 is muxed in the board's device tree but disabled, and the OS image
  is not changed; the OS already lists every overlay in that folder for
  U-Boot.
- **Check:** applied offline with `fdtoverlay` to the development hub's own
  device tree together with its ESPHome overlays, the overlay turns I2C2 on,
  sets the alias and changes nothing else. Not checked on hardware: the bus
  coming up after a restart with the overlay.

## Web UI: stack

### D28. Preact, TypeScript and Vite

- **Decision:** Preact for components, TypeScript in strict mode, Vite to
  build. Packages are installed with npm and a committed lock file.
- **Why:**
  - Preact has been stable for years, is about 5 KB, and uses the same
    component model as React, so React developers are productive in it (1, 2,
    5).
  - SLZB-OS's web UI and `zeesm`'s UI use this stack too: one skill set across
    SMLIGHT's products (4).
  - TypeScript catches at build time what plain JavaScript, as in the 1.x UI,
    finds in the browser.
- **Rejected:** React (the most widely used, but roughly ten times the runtime
  for nothing this UI needs); Svelte, Vue or Solid (sound, but a second UI
  technology in the company); keeping Ant Design, styled-components and
  Tailwind (most of the 1.x UI's 2.5 MiB).

### D29. The browser first, then a few libraries

- **Decision:** use what browsers provide before adding a package: native
  `<dialog>` for modals, native form controls and validation, CSS for layout
  and motion, `Intl` for formatting (D37). Only features available in every
  major browser for at least 30 months are relied on; newer ones are optional
  extras. The runtime packages are listed at the end of this page.
- **Why:** the browser's own parts are the most tested code there is, cost
  nothing to download, and handle focus, keyboard and screen readers correctly
  (1, 2, 5).
- **Rejected:** a component library; hand-written replacements for things a
  small mature library does well. The SLZB-OS UI wrote its own router,
  Markdown renderer and dialogs, and each has gaps.

### D30. Live resources, shared by everything on the page

- **Decision:** one store holds each resource the page shows. The first widget
  that needs a resource subscribes to it over the event connection (D9); later
  widgets share it. Changes pushed by the daemon update every widget. A setting
  is saved through one function: it shows the change at once, sends it, takes
  the daemon's answer as the truth, and puts the old value back with a message
  if the daemon refuses.
- **Why:** no duplicate requests and no polling (3). One save path means errors
  and "restart needed" notices look the same everywhere.
- **Uses:** `@preact/signals` for state.

### D31. Small first load, everything from the hub

- **Decision:**
  - The first load carries the shell and the dashboard. Each other page, and
    each heavy tool (terminal, Markdown), loads when first opened.
  - Files have content-hashed names.
  - Nothing is loaded from the internet: no fonts, no CDN, no remote language
    files.
- **Why:** a fast first screen (the UI's goals), few bytes for nginx to send
  (3), and a hub that works offline (1).
- **Note:** nginx on the hub sends files uncompressed and has no support for
  precompressed ones (measured). The OS image is not changed, so the first
  load is kept small enough not to need compression: under the 400 KB target in
  [vision.md](vision.md#targets), about a tenth of what the 1.x UI downloads
  (2.7 MB).

### D32. Plain CSS with design tokens

- **Decision:** colours, spacing, type sizes, radii and motion timings are CSS
  custom properties (tokens). Component styles are plain CSS scoped per
  component by the build. System fonts. Light, dark and automatic themes.
  Layout uses logical properties, so right-to-left languages work.
  - Each user chooses a text size (small, standard, large: the root font size,
    every type size being in rem), a spacing (comfortable, compact: one factor
    on the spacing tokens; tap targets stay 44 px) and a colour tone (SMLIGHT
    green, teal, blue, violet, pink). `ui/scripts/tones.mjs` generates the
    tones from the green tokens: the same OKLCH lightness and chroma with the
    tone's hue, checked so every text pair keeps 4.5:1 in both themes. Status
    colours and the brand mark do not change.
  - The choices are preferences on the hub (`text_size`, `density`, `tone`),
    applied before the first paint from the browser's last copy.
  - A quiet button beside the account in the sidebar (in the More sheet on a
    phone) switches the theme: each press goes Automatic, Light, Dark.
- **Why:** no styling library runs in the browser (2, 5); tokens keep the look
  consistent and make a re-theme one file (4). The size, spacing and tone
  choices follow SLZB-OS.
- **Rejected:** CSS-in-JS; a utility-class framework; a web font, and with it
  the choice of font family that SLZB-OS has.

### D33. Forms: generated for apps, written for the system

- **Decision:** an app's settings form is generated from the `schema.json` in
  its package, as in 1.x, by one small renderer. System pages (network, Wi-Fi,
  updates, radios, backups) are written by hand with typed data. The daemon
  validates everything; the UI adds only the hints the schema gives. A schema
  field marked `"read-only": true` is the app's own value (Z-Wave JS UI's
  security keys and its `enabled` switch): the form shows it locked with a note
  and never sends it, and the daemon refuses a save that names it (422), as
  1.x's form disables it.
- **Why:** app packages keep working unchanged (1). System pages need flows a
  generic form cannot express well (the UI's goals). A changed Z-Wave key would
  cut off every secure device paired with it.

### D34. Clean page addresses

- **Decision:** page addresses are plain paths such as `/settings/wifi`,
  handled by a small maintained router library. The 1.x UI's `/#/…` addresses
  are redirected to the new ones.
- **Why:** readable, shareable links (the UI's goals). The hub's nginx already
  answers unknown paths with the UI's start page, so no server change is needed
  (1). Old bookmarks keep working.

### D51. A cached old UI finds its way to the new one

- **Decision:** the hub's nginx sends the UI files without cache headers, so a
  browser keeps `index.html` for about a tenth of its age (days) and can open
  an outdated UI after an update. Two measures, both without changing the OS
  image:
  - The 1.x UI, when cached, shows its sign-in form. Its sign-in call
    (`POST /api/v1/login/cookie`) is answered with success and does nothing
    else; the 1.x UI then reloads the page, and the reload fetches the new UI.
  - The new UI knows its package version and compares it with
    `System.ui_version`; when they differ it reloads once.
- **Why:** a browser that has the 1.x UI cached shows the 1.x sign-in form right
  after the update (seen in Chromium on the development hub). Reloading the
  page (F5) also fixes it, but users should not have to know that.
- **OS image:** if it is changed for another reason, its nginx should send
  `Cache-Control: no-cache` for `index.html` and a long lifetime for the hashed
  files under `/assets/`.

## Web UI: product

### D35. Simple by default, advanced on request

- **Decision:** one switch, kept per user, chooses between two levels.
  - **Simple:** health at a glance, plain words, guided steps for common tasks
    (first-run setup, connecting to a network, adding an app, updating),
    sensible defaults, and only the settings most people need.
  - **Advanced:** adds every setting, logs, the terminal, the file manager,
    technical detail on each page, and a command palette to jump anywhere.
  - Both levels use the same pages; advanced reveals more of them.
- **Why:** both audiences are served without two UIs to maintain.

### D36. Every text is translatable

- **Decision:** all text goes through one translation function with typed keys.
  English is the source and is checked at build time: a missing key fails the
  build. Other languages are files on the hub, loaded when chosen. Plural forms
  use the browser's rules for each language. A new language is a new file.
- **Languages:** 31. English, Ukrainian (the two languages of the 1.x UI), and
  the languages SLZB-OS has: Bulgarian, Chinese (simplified), Croatian, Czech,
  Danish, Dutch, Estonian, Finnish, French, German, Swiss German (`de-ch`, from
  German with "ss" for "ß"), Greek, Hebrew (right to left), Hungarian, Italian,
  Japanese, Korean, Latvian, Lithuanian, Norwegian Bokmål (`nb`; `no` maps to
  it), Polish, Portuguese (Portugal), Romanian, Serbian in Latin script
  (`sr-latn`; `sr` maps to it), Slovak, Slovenian, Spanish, Swedish and Thai.
  The language picker is a searchable list.
- **Why:** multilingual by construction (the UI's goals); works offline (1).
- **Open:** who translates and how translations are reviewed. The 29 languages
  besides English and Ukrainian are machine translations: each file passes
  `ui/scripts/check-locale.mjs` (every key, every placeholder, exactly the
  plural forms the language uses), but no native speaker has read them. The
  translators' doubts are listed in [verification.md](verification.md).

### D37. Formats follow the user

- **Decision:** numbers, percentages, dates, times, durations, data sizes,
  units of measurement and currencies are formatted by the browser's `Intl`
  functions according to the user's preferences: language, region, unit system
  (metric or imperial), temperature unit, 12- or 24-hour clock, and time zone.
  The preferences are stored per user on the hub; most of them are settings 1.x
  already stores. The API always carries canonical values (D8).
- **Why:** `Intl` is built into every browser, knows every region's
  conventions and needs no data download (1, 5).

### D38. Polish from speed, live data and motion

- **Decision:** the "wow" comes from these, in order:
  1. it opens at once and never shows a spinner for something already known;
  2. values, progress and status change live, without a reload;
  3. short, purposeful CSS transitions, switched off when the user's system
     asks for reduced motion;
  4. a first-run setup that takes a new hub to a working state in a few steps;
  5. consistency: one set of tokens, one way to save, one way to show errors,
     and an "undo" where a mistake can be reversed.
- **Why:** these impress and stay inside the targets. An animation or
  component library would not (1, 2, 5).

### D39. Accessible, and built for a phone first

- **Decision:** every page meets WCAG 2.2 level AA: keyboard use, visible
  focus, labels for screen readers, sufficient contrast. Layouts are designed
  for a phone and widen for larger screens. Automated accessibility checks run
  in the UI tests.
- **Why:** friendly to everyone (the UI's goals); native controls make most of
  it free (D29).

## Security

### D40. Deny by default

- **Decision:** every route and every WebSocket requires a signed-in user unless
  it is on a short public list: health, sign-in, the public part of `system`,
  and the two API v1 addresses of D48 and D51. The check is one layer in front
  of all routes, not a per-route option.
- **Why:** in 1.x reboot, OS install, network changes and the web terminal have
  no check, because each route has to opt in.

### D41. Passwords and sessions

- **Decision:**
  - The daemon verifies the Argon2 and legacy bcrypt hashes already in the
    database, so existing accounts keep their passwords. One check runs at a
    time.
  - A hub that does not require sign-in on 1.x does not require it after the
    update. The existing setting is honoured; the UI shows clearly that the hub
    is open and offers to set a password.
  - Sessions are random tokens in a cookie that scripts cannot read
    (`HttpOnly`, `SameSite=Strict`, sent only to `/api/v2` and `/ws/v2`). The
    hub stores only their hashes. Signing out is a call to the hub and takes
    effect at once. After the update everyone signs in once more.
  - The UI contains no built-in account. A new hub starts with a first-run
    setup.
  - The factory account (`admin@smlight.tech`) has a password the 1.x UI
    carries, so anyone can know it. While sign-in is on it does not open the
    hub: turning sign-in on gives it the owner's new password when the owner
    keeps that name, otherwise a random one, and keeps its factory hash aside
    in `smhubd.db`. Turning sign-in off gives the hash back, so the 1.x UI,
    which signs itself in with that account on an open hub, works again after
    going back. The user-name field starts empty, with `user@localhost.com` as
    an example.
  - Every account can do everything, as in 1.x. The `admin` flag is stored and
    shown but grants nothing extra: when sign-in is turned on, the 1.x UI
    creates the owner's account as a non-administrator, so a check on the flag
    would lock those owners out of their own hubs.
- **Why:** existing setups keep working (D3). In 1.x every hub signs sessions
  with the same key from the package, a session cannot be revoked, and the UI
  carries a default account.

### D42. The local socket checks who is calling

- **Decision:** the socket is created with restricted permissions and the daemon
  reads the caller's user ID from the kernel. Only root and the device user are
  accepted.
- **Why:** in 1.x the socket is writable by everyone and any local process is
  treated as the system user.

### D43. Protections in the browser

- **Decision:**
  - A strict content security policy, set by a meta tag in the built page
    because nginx is not changed: scripts only from the hub itself; styles from
    the hub and inline, because the terminal (xterm) creates its own style
    elements; connections only to the hub.
  - A request that changes something is refused when its `Origin` names
    another site.
  - Text from packages and changelogs is shown through a sanitiser, never
    inserted as raw HTML.
  - An embedded app's page never receives the hub's session: the cookie is
    sent only to the hub's API paths (D41), and the frame sends no referrer.
- **Why:** package descriptions, changelogs and icons come from a feed; the UI
  must not trust them with its session.

### D55. API tokens for other programs

- **Decision:** a signed-in user makes named tokens (`/tokens`); a program
  sends one as `Authorization: Bearer`. The daemon keeps only its hash, acts as
  the user who made it, and refuses a token on sign-in, account, token and
  terminal routes. A token's value is shown once.
- **Why:** Home Assistant and scripts need the API on hubs with a password
  (infrared, buzzer, light); a shared password in their configuration would be
  worse.

## Quality

### D44. A bug fails one request

- **Decision:** in the daemon, `unwrap`, `expect` and `panic!` are denied by
  lint outside tests, and `unsafe` is forbidden in our crates. Panics unwind
  and are caught at the request and task boundaries; OpenRC's supervisor
  restarts the process if it ever dies. In the UI, a failing page shows an
  error in its own area and the rest keeps working.
- **Why:** `zeesm` has 3,180 `unwrap()` calls and aborts on panic, so any one of
  them takes the daemon down (1).

### D45. Logging

- **Decision:** `tracing`, written to stderr, which the package's init script
  sends to `/var/log/smhub-services.log`. Level `info` by default (`RUST_LOG`
  sets another); there is no per-request access log.
- **Why:** logs live in RAM on the hub (2).

### D46. Tests

- **Decision:**
  1. **Contract tests:** every answer of the daemon is checked against the API
     description (D7).
  2. **Effect tests:** the same setting change through the Python backend and
     through the daemon, each on a copy of a prepared root directory, must
     leave the same config files and run the same commands.
  3. **Upgrade tests:** a hub with a real setup made on 1.x (accounts, apps
     with data, network, a backup) is updated to 2.0 and checked against the
     list in [vision.md](vision.md#what-must-not-break); then 1.x is installed
     again and it is checked once more (`tests/hub/downgrade_upgrade.sh`).
  4. **UI tests:** a real browser against the real daemon, on a hub or on a
     development machine with `--root` (`ui/tests/pages.spec.ts`: every page
     at both levels, phone and desktop; `ui/tests/clickthrough.mjs`: every
     action with real clicks). There is no hand-written mock backend to drift
     from the real one. Accessibility checks run inside them (D39).
  5. **On-hub checks:** the scripts in `tests/hub/` (API write paths,
     hardware, service states) and the measurements of D47.
- **Not in place:** the contract tests (1) and the effect tests (2). Their
  ground is covered in part: the UI's types are generated from the description,
  `tests/hub/api_write_test.py` exercises every write path on a hub (app config
  files included, byte for byte), and Rust unit tests check the files the
  daemon writes.
- **Why:** tests 2 and 3 prove the compatibility contract (D3). The Python
  backend remains the specification for what happens on the OS
  ([old-backend-behaviour.md](reference/old-backend-behaviour.md)). The SLZB-OS
  UI tests run against a 3,700-line mock that is a second implementation of its
  API.
- **Results:** what is checked on hardware, and how, is in
  [verification.md](verification.md).

### D47. Targets are enforced by measurement

- **Decision:** scripts measure the targets in [vision.md](vision.md#targets)
  for every release, on a hub for the daemon (`tests/hub/measure.sh`,
  `boot_timing.sh`, `compare_run.sh`, `compare.sh`) and in a real browser for
  the UI (`ui/tests/load-timing.mjs`, `compare-load.mjs`). The results are
  stored in this repository: [vision.md](vision.md#targets),
  [README.md](../README.md#measured) and `docs/report/index.html`.
- **Why:** numbers drift unless someone looks at them (2, 3).

### D48. The daemon tells the hub when it is ready

- **Decision:** at the moment it can serve requests, the daemon sends the
  readiness message that OpenRC's supervisor already understands. It also
  answers the address that the OS image's own readiness check asks
  (`/api/v1/login/user-session`), with the answer that check accepts. OpenRC
  0.63's `supervise-daemon` leaves its readiness socket behind when the service
  stops, and the next start then fails, so the package's init script removes the
  stale socket before each start.
- **Why:** the OS image's check polls every 15 s while the backend starts, and
  every app waits for it. The daemon is up before the check first asks, so apps
  are released at once, with no change to the OS image (3). Measured on the
  development hub: the daemon is ready 170–440 ms after it starts, about 4 s
  into boot.

## Direct dependencies

Versions are the ones in `Cargo.lock` and `ui/package-lock.json`.

### Daemon (Rust crates)

| Crate | Version | Used for | Why it qualifies |
|---|---|---|---|
| `tokio` | 1.54.1 | async runtime, timers, processes, signals | the standard Rust runtime; 1.x since 2020 |
| `axum` (on `hyper`), `tower-http` | 0.8.9 (`hyper` 1.12.0), 0.6.11 | HTTP server, routing, WebSocket, request limits and time-outs | from the Tokio project; the most used Rust web stack |
| `futures-util`, `bytes` | 0.3.34, 1.12.1 | WebSocket streams, request bodies | from the same stack; also pulled in by `axum` |
| `serde`, `serde_json` (`preserve_order`) | 1.0.229, 1.0.154 | JSON; key order kept so app config files keep their order | the standard; 1.x since 2017 |
| `rusqlite` (bundled SQLite) | 0.40.2 (SQLite 3.53.2) | database | thin binding over SQLite; in use since 2014 |
| `argon2`, `bcrypt`, `pbkdf2`, `sha2`, `base64` | 0.5.3, 0.17.1, 0.12.2, 0.10.9, 0.22.1 | password hashes, session token hashes, mosquitto password file | the same crates ship on hubs inside 1.x's `smhub_rs` |
| `getrandom` | 0.3.4 | session tokens, operation identifiers | the standard source of random bytes |
| `nix` | 0.31.3 | terminals, socket credentials, process groups, the modem's serial settings | the usual safe wrapper over Linux system calls; since 2015 |
| `inotify` | 0.11.5 | following log files and OpenRC/opkg state without polling | small wrapper over the kernel interface |
| `tracing`, `tracing-subscriber` | 0.1.44, 0.3.23 | logging | from the Tokio project |
| `serde-saphyr` | 1.3.0 | reading and writing app config files in YAML (`zigbee2mqtt`); reading the serial bridge's entries (D58) | **the weak spot:** a stable 1.x release, but young; its parser, `granit-parser`, is the same author's fork of the `saphyr` parser. It sits behind `smhub-sys`'s `appconfig` module, and the API write test on a hub checks a byte-exact round trip of Zigbee2MQTT's `configuration.yaml` |
| `snow` (ChaCha20-Poly1305, SHA-256, Curve25519 only) | 0.10.0 | the encryption of ESPHome's API (Noise NNpsk0), D54 | the standard Noise implementation in Rust since 2017, on the RustCrypto primitives; the AES and BLAKE2 parts are left out |
| `spidev` | 0.7.1 | the light ring's SPI bus when Linux owns it: the bus speed needs an ioctl, which the daemon may not write itself (no `unsafe`) | from the rust-embedded working group; brings its own older `nix` 0.29 |

Not used, on purpose: an ORM, an HTTP client library (the system `curl` makes
the few HTTP requests, D19), a TLS library, a signed token library, a command
line framework, a D-Bus library (D24), a GPIO crate (D25), a serial port crate
(the modem's AT port is a plain file, set up with `nix`), a date library
(timestamps are formatted by a small function in `smhub-model`), a time zone
database (the OS's `zoneinfo` is read instead).

In numbers: 149 crates in the daemon's dependency tree for the hub target
(`cargo tree -p smhubd --target riscv64gc-unknown-linux-musl -e normal,build`,
the six workspace crates included); the stripped static binary in the
`smhub-services` 2.0.3 package is about 6.2 MB.

### Web UI (npm packages that reach the browser)

| Package | Version | Used for | Why it qualifies |
|---|---|---|---|
| `preact` | 10.29.8 | components | stable for years (10.x since 2019); about 5 KB. 11.x (September 2026) is left until it has a track record |
| `@preact/signals` | 2.11.4 (brings `@preact/signals-core` 1.14.5) | state (D30) | from the Preact project; 2.x since December 2024 |
| `wouter-preact` | 3.13.0 (brings `regexparam` 3.0.0) | routing (D34) | small, maintained, widely used; 3.x since February 2024 |
| `lucide-preact` | 1.69.0 | icons, named imports only | the maintained successor of Feather icons; each icon is its own module, so only the icons used are bundled (about 120) |
| `@xterm/xterm` and `@xterm/addon-fit` | 6.0.0 and 0.11.0 | web terminal; in the terminal page's file only (333 KB, never in the first load) | the terminal used by VS Code |
| `marked` and `dompurify` | 18.1.3 and 3.4.17 | changelogs and release notes in Markdown, sanitised; one file loaded on demand (80 KB) | both long-established and widely used |

Build and test tools, which never reach the hub: Vite 7.3.7 (it bundles with
Rollup 4 and transforms with esbuild 0.28), TypeScript 5.9.3, openapi-typescript
7.13.0 (types from `api/openapi.yaml`), Playwright (`@playwright/test` 1.65.0)
and axe (`@axe-core/playwright` 4.14.0). Vite 7 rather than 8: Vite 8 (March
2026) replaced Rollup with the much younger Rolldown, while 7.x still gets
fixes (priority 1). TypeScript 5.9 rather than 6 or 7: openapi-typescript
requires TypeScript 5, and 7 is the newly ported compiler. No Preact plugin for
Vite: `@preact/preset-vite` brings Babel and a hot-reload layer; esbuild
compiles the JSX by itself, and during development a change reloads the page.

Not used, on purpose: a component library, a styling library, a data-fetching
library, a translation library, a date library, an animation library, a web
font.
