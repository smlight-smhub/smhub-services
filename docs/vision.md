# Vision

What `smhub-services` and `smhub-ui` 2.0 are for, who they serve, the priorities it follows and the
targets it is measured against.

## What it is

Three things that are designed together and live in one repository:

1. **`smhubd`**, one small Rust program that does everything the 1.x Python
   backend does: it keeps the hub's settings, installs and controls apps,
   configures the network, runs OS updates and talks to the hub's hardware.
2. **A web UI** that people enjoy using, whether or not they are technical.
3. **API v2**, the contract between the two, written down in one file.

They ship as the same two opkg packages as 1.x (`smhub-services` and
`smhub-ui`) and arrive on a hub as an ordinary update.

There is one hard requirement: **the update must not break an existing user's
setup.** Everything else about 1.x's backend, frontend and API is free to
change.

A user notices that the hub is ready much sooner after power-on, that the UI
opens at once and feels alive, and that apps have more memory.

## Why

What 1.x costs, measured on an SMHUB Essential with OS 1.0.2. 2.0's results
are under [Targets](#targets).

**Backend**

- Importing the Python backend takes 32 s of CPU on the hub's single core before
  any of its own start-up logic runs.
- It holds 84 MiB of private memory after start and 108 MiB after some use,
  half of all process memory on a hub with no apps running.
- Every app waits for it at boot. In a logged boot the backend process starts
  at 89 s and the hub reports ready at 177 s.

**Web UI**

- It is one 2.5 MiB JavaScript file with no code splitting. The hub sends it
  uncompressed and without cache headers.
- A page load makes about 25 requests, several duplicated, some one after
  another. One of them makes the hub call the update server on the internet.
- The page asks Google for a font, which stalls on a network without internet.
- Some status is polled instead of pushed.

**API**

- State-changing actions are GET requests; many routes have no sign-in check;
  the session cookie is readable by scripts; the UI works out the hub's features
  by reading GPIO lines itself.

## Who the web UI is for

Two kinds of people use an SMHUB, often in the same household:

- **Someone who wants a smart home that works.** They want to see that
  everything is fine, add an app, connect to Wi-Fi and update the hub, in plain
  words and a few taps.
- **Someone technical.** They want the details: logs, a terminal, files, every
  setting, and fast ways to reach them.

The UI serves the first by default and the second on request, without making
either feel they are using the other's product.

## What the web UI must be

1. **For both audiences.** Simple by default, advanced on request.
2. **Multilingual.** Every text can be translated; adding a language needs no
   code change.
3. **Local in its formats.** Numbers, dates, times, units of measurement and
   currencies are shown the way the user's region writes them, and the user can
   choose units (for example °C or °F).
4. **Modern and very easy to use.** Clear, consistent, forgiving of mistakes,
   and as good on a phone as on a desktop.
5. **Impressive.** It opens instantly, shows live values without reloading,
   moves smoothly, and explains what it is doing.

The fifth goal is met through speed, live data and care in the details, not
through heavy libraries. That keeps it inside the priorities below.

## Priorities in practice

Five priorities, in order. Each one is turned into rules here so that a design
choice can be checked against it.

### 1. Stable, mature system components and libraries

- **Daemon:** use what the OS already ships and trusts: nginx, stunnel, SQLite,
  OpenRC, opkg, NetworkManager, RAUC, the kernel's own device interfaces.
- **Web UI:** use what browsers already ship: native dialogs and form controls,
  CSS layout, and the browser's own number, date and unit formatting. Only
  browser features that have been in every major browser for at least 30 months
  are relied on; newer ones are optional extras.
- **Both:** a library is added only if it is widely used, actively maintained
  and released as stable. No pre-release versions, no nightly compiler features.
  Every direct dependency is listed in [decisions.md](decisions.md) with its
  reason. Fewer is better.
- Prefer boring designs. One process, one database file, plain SQL, plain HTTP,
  plain CSS.

### 2. Smallest RAM use

- **Daemon:** no interpreter. Nothing large is held in memory: uploads,
  downloads, backups and logs are streamed. Every queue, buffer, cache and
  connection count has a stated limit. As few threads as the work needs.
- **Web UI:** a small first download. Long lists (logs, files) are capped or
  drawn in windows, so the browser tab does not grow without bound.

### 3. Smallest CPU use

- **Daemon:** nothing polls. No timer fires while nobody is connected, apart
  from documented housekeeping. No process is spawned per request without a
  short cache in front of it. Slow work never blocks other requests.
- **Web UI:** every request it sends costs the hub CPU, so it sends few. State
  arrives over one connection and is shared by everything on the page. Nothing
  is asked twice, nothing is polled, and nothing is fetched from the internet.

### 4. Future-proof and expandable

- The API is versioned and described in one contract file that both sides are
  checked against.
- Hardware models are described in one table. A new model is one row, and the
  UI learns what the hub can do from the API, not from its own rules.
- All access to the operating system goes through one module.
- A new settings page, app integration or language is an addition, not a
  change to existing code.
- The layout is ready for right-to-left languages, and the daemon does not
  assume one CPU core.

### 5. Smallest flash size

- One statically linked binary for the daemon.
- A UI package smaller than 1.x's, including its languages.
- This is the lowest priority: it never overrides the four above.

### When priorities conflict

| Conflict | Choice | Because |
|---|---|---|
| A newer, faster library against an older, proven one | The proven one | 1 over 3 |
| A component library with ready-made widgets against native controls and our own small kit | Native controls and a small kit | 1, 2 and 5 |
| An animation library for visual effect against CSS transitions | CSS | 1 and 2; the effect is kept |
| Caching parsed files in the daemon against re-reading them | Re-read, unless measured otherwise | 2 over 3 |
| Terminating TLS inside the daemon against keeping stunnel | Keep stunnel | 1 and 2 over 5 |
| Shipping every language on the hub against fetching them from a server | Ship them | 1 over 5: the hub must work offline |
| A plugin system for future features against plain modules | Plain modules | 1 over 4 |

## Targets

Measured on a hub, on every release. A release that misses one is not shipped
without a written reason. The values for 2.0 are from the development hub (SMHUB
Essential, OS 1.0.2).

### Daemon

| What | Target | 1.x (Python) | smhubd 2.0 |
|---|---|---|---|
| Process start to "API ready" | 1 s or less | over 70 s at boot | 0.17–0.50 s; about 4 s into boot |
| Private memory, idle, UI closed | 8 MiB or less | 84–108 MiB | RSS 6.8 MiB, of which 1.2 MiB anonymous (the hub's kernel has no `smaps`; the rest is the binary's own pages) |
| Private memory, one UI session open | 12 MiB or less | — | RSS 5.4 MiB, 1.5 MiB anonymous |
| Threads when idle | 4 or fewer | 12 | 2 |
| CPU when idle with no client | under 0.1 % over 10 minutes | not measured | 0.002 % (1 tick in 600 s) |
| Stripped binary | 10 MB or less | — | 6.2 MB, static (builder image); the release package `smhub-services`, with the radio flasher, is 5.8 MB |
| Response to a simple GET, on the device | 5 ms or less | 9–16 ms | 3.5 ms direct, 5.6 ms through nginx (median of 20) |

Checking a password uses Argon2, which needs 19 MiB for under a second. That
buffer is outside the memory targets, and only one check runs at a time.

### Web UI

| What | Target | 1.x | smhub-ui 2.0 |
|---|---|---|---|
| First load, as the hub sends it (uncompressed) | 400 KB or less | 2.5 MiB of JavaScript | 273 KB in 4 files |
| Requests before the dashboard is usable | 6 or fewer, plus one WebSocket | about 25 | 6 plus one WebSocket |
| Requests while a page is open and nothing changes | none | some pages poll | none in 60 s |
| Requests to the internet caused by opening a page | none | a font request; an update check on the hub | none |
| Time to a usable dashboard on a local network | 1 s or less | not measured | **not met on a first visit.** Measured over a VPN, not the local network: with the browser's and the hub's caches warm 0.25 s; on a first visit the page and its first cards show at 0.5 s and the last cards fill at about 1.7 s, because the hub works out network, service and app state cold (about 1.2 s on its single core) |
| Visible response to a tap or click | within 100 ms | — | not measured separately; changes are shown at once and confirmed by the hub |
| UI package on flash, with two languages | 1.5 MiB or less | 2.7 MiB | 0.97 MB with English and Ukrainian; 3.2 MB with all 31 languages (the `smhub-ui` package is 0.7 MB) |
| Each further language | 40 KB or less | — | **not met:** 66–121 KB per language as built (Ukrainian 100 KB; Cyrillic takes two bytes a letter, Thai three) |
| Accessibility | WCAG 2.2 level AA on every page | not assessed | axe finds no WCAG 2.2 A/AA problem on any page, phone and desktop |

## What must not break

This is the only compatibility requirement. It is checked on real upgraded hubs
before 2.0 reaches the stable channel ([plan.md](plan.md)).

| What the user has | What we guarantee |
|---|---|
| Accounts and passwords | Existing accounts sign in with their existing passwords. A hub that did not ask for a password still does not |
| Settings | The database keeps its file, its tables and its data. Every saved setting keeps its meaning |
| Apps | Installed apps, their data and their settings are untouched. App packages need no change; their settings forms still come from each package's `schema.json` |
| Network | Ethernet, Wi-Fi, VPN and LTE configuration is untouched |
| Radios | Zigbee, Thread and Z-Wave networks and radio firmware are untouched |
| App updates | Apps install and update from the same feeds and channels, the same way |
| OS updates | Two slots with automatic fallback, core packages reinstalled after an OS update, the same channels |
| Backups and factory reset | A backup made before the update restores after it. Factory reset behaves the same |
| Addresses | The hub's web address and each app's web address stay the same |
| Going back | Installing the previous package versions gives a working hub again |

## What may change

- How the web UI looks, how it is organised, and its page addresses.
- The whole API.
- Every internal solution of 1.x's backend and frontend.
- How things are done, as long as the result on the hub is the same for the
  user.

## Out of scope

- Removing something users can do in 1.x. Everything possible in 1.x stays
  possible; where and how may change.
- New hub capabilities as a condition for replacing 1.x. They are welcome where
  they serve the UI's goals, but none is required.
- Rewriting the radio flashing protocols. Flashing stays in Python, started only
  when someone flashes a radio.
- Changing the OS image. 2.0 works with the image as it is; see
  [architecture.md](architecture.md#what-the-os-image-expects).
- Splitting the daemon into several services, a plugin system, or a scripting
  engine.
- A native mobile app.

## Principles

1. Measure on the hub and in a real browser. An estimate is not a result.
2. One contract. The daemon and the UI are both checked against the same API
   description.
3. One owner per device: a serial port or GPIO line is held by one program, and
   everything else goes through that program.
4. A bug fails one request, never the daemon; one page, never the UI.
5. Deny by default. A route is private unless it is listed as public.
6. The hub works without the internet. Nothing the UI needs comes from outside.
7. The user's setup is the contract. It is tested by upgrading real hubs, and
   by going back.
