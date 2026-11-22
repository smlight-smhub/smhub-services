# AGENTS.md

How to work in this repository. It is written for AI coding agents and holds
for people too. [README.md](README.md) says what the project is; this file says
how to change it without breaking anything.

## What this is

The core software of the SMLIGHT SMHUB, a home-automation hub (Sophgo SG2000:
one 1 GHz RISC-V core for Linux, about 490 MiB of RAM; OpenRC, BusyBox, opkg).
Three parts, one repository:

- `smhubd`, a Rust daemon (`crates/`), started by OpenRC as
  `/opt/bin/smhub-services`. It owns the database, talks to the OS and the
  hardware, and serves API v2.
- The web UI (`ui/`, Preact and TypeScript, built with Vite), served by nginx
  on the hub.
- API v2 (`api/openapi.yaml`), the contract between them.

It is version 2.0 of the packages `smhub-services` and `smhub-ui`. It replaces
version 1.x, a Python backend and a React UI, which runs on hubs that are
already in people's homes.

## Read first

1. "What must not break" in [docs/vision.md](docs/vision.md) and
   "Compatibility with existing hubs" in [docs/architecture.md](docs/architecture.md).
2. [docs/decisions.md](docs/decisions.md): each decision (`D1`, `D2`, …) with
   its reason. Code follows them.
3. [api/openapi.yaml](api/openapi.yaml) for anything the UI and the daemon
   exchange.
4. [docs/reference/old-backend-behaviour.md](docs/reference/old-backend-behaviour.md)
   when a change touches what is stored on a hub or written to the OS: it
   records what 1.x did, and 2.0 must stay compatible with it.

## The one hard requirement

An update must not break an existing user's setup. The database keeps its
structure and data; apps and their settings are untouched; updates, backups
and factory reset behave as before; going back to 1.x works. The fixed formats
are listed in `docs/architecture.md` ("Compatibility with existing hubs").
Nothing in that list changes format or meaning. Everything else about 1.x's
backend, UI and API is free to change.

## Priorities

In this order; the higher one wins a conflict. They apply to the daemon and
the UI.

1. Stable, mature system components and libraries
2. Smallest RAM use
3. Smallest CPU use
4. Future-proof and expandable
5. Smallest flash size

The UI must also be easy for non-technical users, translated, with local
formats, and fast. "Impressive" comes from speed, live data and care, not heavy
libraries (`docs/decisions.md`, D38).

## Set up

- Docker. Everything Rust builds in the builder image, which pins Rust 1.95
  with the RISC-V musl target, Zig (the C compiler for the bundled SQLite),
  cargo-zigbuild and Node.js:

  ```bash
  docker build -t smhub-services-build:2 -f packaging/Dockerfile.build packaging
  ```

  `packaging/cargo.sh <command>` runs a command in that image with the
  repository mounted at `/src` and the cargo caches in Docker volumes.
- Node.js 20.19 or newer for the UI (`cd ui && npm ci`).
- On Windows, use Git Bash for the shell scripts. Files are LF
  (`.gitattributes`).

## Commands

| What | Command | Must |
|---|---|---|
| Rust tests | `packaging/cargo.sh cargo test --workspace` | pass |
| Rust lints | `packaging/cargo.sh cargo clippy --workspace --all-targets` | no warnings |
| UI types | `cd ui && npm run typecheck` | pass |
| UI build | `cd ui && npm run build` | pass (also prints sizes and translation coverage) |
| Translations | `cd ui && node scripts/check-locale.mjs <code>` for every file in `src/text/locales` | pass for each |
| Colour tones | `cd ui && node scripts/tones.mjs` after changing a colour token | pass (keeps 4.5:1 contrast) |
| API types | `cd ui && npm run gen:api` after changing `api/openapi.yaml` | regenerates `src/api/schema.d.ts` |
| Browser tests | `cd ui && npm test` (Playwright, every page at both UI levels, accessibility) | pass on a hub |
| Click-through | `cd ui && node tests/clickthrough.mjs [url]` (every action with real clicks) | pass on a hub |
| Packages | `SEED_DB=<seed database> packaging/build-ipk.sh` | writes `out/*.ipk`; versions follow [Version numbers](docs/architecture.md#version-numbers) |

The browser tests and scripts find the daemon through `SMHUB_URL` (default
`http://127.0.0.1:8000`). Playwright empties `ui/test-results/` when it
starts; keep other output elsewhere.

## Run without a hub

The daemon runs on a development machine. `--root` puts every OS path under a
directory of your choice, and a missing database is created with the full
schema:

```bash
docker run --rm -p 127.0.0.1:8000:8000 -v "$PWD:/src" -w /src smhub-services-build:2 sh -c 'mkdir -p /tmp/r && cargo run -p smhubd -- --root /tmp/r --listen 0.0.0.0:8000 --socket none --db /tmp/r/backend.db --upload-dir /tmp/r/up'
```

```bash
cd ui && npm run dev
```

Then open http://localhost:5173. What to expect:

- A fresh hub: the first-run setup, no password (the UI says anyone on the
  network can change the hub), no apps.
- Hardware the machine does not have (LEDs, buzzer, light ring, radios, GPIO)
  reports itself as unavailable; the pages still render.
- `--bin-dir DIR` makes the daemon look for commands in `DIR` first, so a test
  can stand in for `nmcli`, `rc-service`, `opkg` and the like.
- `smhubd --help` lists every flag.

Anything that depends on real hardware or on the hub's OS image (radio
flashing, network changes, OpenRC services, packages) is verified on a hub; see
below.

## Where a change goes

Daemon crates, and what each may depend on. Dependencies only point down.

| Crate | Holds | May depend on |
|---|---|---|
| `smhubd` | start-up, flags, wiring, signals | all below |
| `smhub-api` | routes, sign-in, WebSockets, API v2 shapes, hardware tasks | `smhub-auth`, `smhub-store`, `smhub-sys`, `smhub-model` |
| `smhub-sys` | everything that touches the OS: files, commands, devices | `smhub-auth`, `smhub-model` |
| `smhub-store` | SQLite access, the 1.x schema | `smhub-model` |
| `smhub-auth` | password hashes, sessions | `smhub-model` |
| `smhub-model` | plain types, error codes, the hardware model table; no I/O | — |

Typical paths:

- **A new or changed API call:** `api/openapi.yaml` first (D7), then
  `npm run gen:api` in `ui/`, then the route in `crates/smhub-api`, the OS work
  in `crates/smhub-sys`, then the UI. Never edit `ui/src/api/schema.d.ts` by
  hand.
- **A new UI text:** add the key to `ui/src/text/en.ts` and to every file in
  `ui/src/text/locales/`, then run `check-locale.mjs` for each language. Every
  text goes through the translation function; every shown value (numbers,
  sizes, dates, units) goes through the formatting module.
- **A new page:** one folder under `ui/src/pages/`, registered in
  `ui/src/shell/routes.ts`; build it from `ui/src/kit/`.
- **A new dependency** (Rust crate or npm package): add a row to the tables at
  the end of `docs/decisions.md`; it must pass the policy in D13.
- **Going against a decision:** change `docs/decisions.md` with the reason
  first. Do not write code that contradicts it.

## Rules

Daemon:

- No `unwrap`, `expect`, `panic!` or `unsafe` outside tests.
- Every queue, buffer, cache and connection count has a limit, listed under
  "Limits" in `docs/architecture.md`.
- Only `smhub-sys` touches the OS. Files and formats that 1.x also reads or
  writes stay in 1.x's format.

Web UI:

- Use a browser feature before adding a package.
- Nothing is loaded from the internet: no CDN, fonts, images or scripts from
  other hosts.
- Build from the kit's components and the design tokens; no new colours
  outside the tokens.

Both:

- Claims about speed, size or memory are measured, on a hub or in a real
  browser, not estimated.
- Keep changes small and on one topic. Do not reformat code you did not change.

## Testing on a real hub

Only on a hub you own or may change. Hub scripts run as root on the hub, and
each puts back what it changes:

| Script | Checks |
|---|---|
| `tests/hub/api_write_test.py` | the API's write paths |
| `tests/hub/hardware_test.py` | light ring, LEDs, buttons, infrared, tokens (sound only with `--sound`) |
| `tests/hub/services_compare.sh`, `services_states.sh` | service states against `rc-status` |
| `tests/hub/downgrade_upgrade.sh` | 2.0 → 1.x → 2.0 with data kept (run it detached: 1.x's backup stops every app service, Tailscale included) |
| `tests/hub/compare_run.sh`, `boot_timing.sh`, `compare.sh`, `measure.sh` | the measurements in `docs/report/`; `compare_run.sh` and `boot_timing.sh` run on your computer and reach the hub through a helper named in `SMHUB_SSH` (it runs `--root COMMAND` and `--put FILE DEST`) |

To try a build: copy both packages to the hub and run
`opkg install --force-reinstall ./smhub-services_*.ipk ./smhub-ui_*.ipk` as
root. Keep your way in: do not stop the SSH server, the network or a VPN you
are connected through, and do not save a network change on the interface you
are using. Write down what you change and put it back.

## Done means

- [ ] `cargo test` and `cargo clippy` clean; UI typecheck and build pass;
      `check-locale.mjs` passes for every language when texts changed.
- [ ] `api/openapi.yaml` and the generated types match the code.
- [ ] New behaviour has tests next to the code; UI changes pass the browser
      tests.
- [ ] Nothing in the compatibility list changed; if a stored format is
      touched, going back to 1.x was checked.
- [ ] Docs that describe the changed behaviour are updated in the same change.
- [ ] No secrets, real addresses or personal data in the diff (below).

## Commits and pull requests

- One topic per commit. The subject says what changed, in the present tense,
  prefixed with the area when that helps (`Serial bridge: …`).
- The body says what was wrong or missing, why, what the change does, and how
  it was checked (commands, and on which hardware). Say what was not checked.
- A pull request lists the same, plus the results of the commands above.
- Outside contributors sign the contributor agreement before their first pull
  request is merged ([CONTRIBUTING.md](CONTRIBUTING.md)). An agent working for
  a contributor tells them so; it does not sign for them.

## License

The repository is licensed under PolyForm Noncommercial 1.0.0 with an
additional permission for SMLIGHT hardware ([LICENSE.md](LICENSE.md));
`helpers/radio-flash/` and `packaging/vendor/` are GPL-3.0.

- New dependencies need a permissive license (such as MIT, Apache-2.0, BSD,
  ISC, Zlib, BSL-1.0 or the Unlicense) or MPL-2.0, as every current crate and
  npm package has. GPL, AGPL and LGPL code goes only into `helpers/radio-flash/`
  or `packaging/vendor/`, never into the daemon or the UI; a dual license with
  a permissive option is fine.
- Do not copy code from other projects unless their license allows it under
  these terms; say where it came from in the commit.
- Keep the `SPDX-License-Identifier` lines and the `Required Notice` line.

## Public repository: secrets and private data

This repository is public. Before every commit:

- **No secrets.** No passwords, tokens, API keys, private keys, package feed
  credentials or `.env` files. A hub's login belongs in a git-ignored
  `.credentials` file, never in code or docs.
- **No private data.** No real IP or MAC addresses, serial numbers, host names
  of real machines, Wi-Fi names, names of people or local paths. Use example
  values: `192.168.1.20` (LAN), `100.64.0.10` (VPN), `02:00:00:00:00:01`
  (MAC), `<hub address>`.
- **Screenshots** only with `ui/tests/screenshots.mjs`: it replaces the hub's
  addresses with example values, and `SCREENSHOT_HIDE_APPS=name,…` leaves out
  apps that are not released. Look at every picture before committing it.
- **Security issues** are reported privately through GitHub's vulnerability
  reporting (the repository's Security tab), not in a public issue.
