# Plan

Where `smhub-services` and `smhub-ui` 2.0 stand, what they must show before
hubs on the stable channel get them, and what comes after.

## Where 2.0 stands

- `smhub-services` and `smhub-ui` 2.0.3 (package version 2.0.3-1) do
  everything a user can do in 1.x, plus the hub's own hardware: buzzer, light
  ring and LEDs, infrared remote and buttons. What is checked, and how, is in
  [verification.md](verification.md).
- Hubs get 2.0 from the development channel of SMLIGHT's package feed, built
  by SMLIGHT's package builder from a commit of this repository, or as a
  pre-release from GitHub ([README](../README.md#install-on-a-hub)).
- Hubs on the stable channel keep 1.x. Going from 2.0 back to 1.x keeps every
  setting and the data.
- The UPS board page is in development builds only
  ([peripherals.md](peripherals.md#ups-board)).

## Before the stable channel

2.0 replaces 1.x in a single release (D5). Before that release, it proves the
compatibility contract (D3) beyond the development hub:

- Upgrade tests on hubs with real, varied setups: each model family (SMHUB,
  Nano, Nano MR), with and without sign-in, with apps holding data, with Wi-Fi,
  VPN and LTE configured.
- Every row of [what must not break](vision.md#what-must-not-break) checked
  after the update, and again after going back.
- An update from the oldest supported 1.x version.
- A backup from 1.x restored on 2.0, and the reverse.
- An OS update and a factory reset with 2.0 installed.
- Browsers other than Chromium: Firefox and Safari, on a computer and on a
  phone.
- A period on the development channel with real users.

**Must show:** every target in [vision.md](vision.md#targets) met; every row of
the compatibility list passed on every model family; no open fault that loses
data or a setting.

## The stable release

- 2.0 is published to the stable channel as the next major version of
  `smhub-services` and `smhub-ui`.
- The 1.x versions stay in the feed for going back.
- `fastapi-backend`, `system-config` and `frontend` (1.x) receive only serious
  fixes for an agreed period, then are archived.

## While 1.x and 2.0 both ship

- 1.x gets fixes; new features go into 2.0.
- The database keeps the structure 1.x gives it (D21). If it must change, 1.x
  and 2.0 change together, in the same release.
- Nothing in [the compatibility list](architecture.md#compatibility-with-existing-hubs)
  changes format or meaning, on either side.

## After the stable release

Each is a separate decision:

- App web UIs under the hub's own address instead of separate ports.
- The OS layer split into a small privileged helper.
- Schema changes for new features, owned by the daemon (D21), once 1.x is no
  longer supported.
- Changes to the OS image that would make the hub faster
  ([architecture.md](architecture.md#what-the-os-image-expects)). 2.0 needs
  none of them.

## Risks

| Risk | Effect | Answer |
|---|---|---|
| One release replaces everything | a fault reaches every hub at once | the development channel first; upgrade tests on real setups; going back is tested and documented |
| An existing setup breaks on update | the one thing that must not happen | the compatibility list drives the tests; the checks before the stable channel are dedicated to it |
| 2.0 lacks something 1.x has | a capability is lost at the release | the list of what 2.0 does not do yet, in [verification.md](verification.md) |
| Config files written differently from 1.x | an app fails to start after a settings change | tests per app on a hub |
| A hub updates from a very old 1.x version | its database is not at the expected revision | the remaining migration steps ship as SQL (D21); tested from the oldest supported version |
| Something else calls API v1 | it breaks at the release | the open question below; a small compatibility layer if needed |
| Hardware paths cannot be tested on a PC | a regression reaches a hub | the checks in `tests/hub/` on a hub before each release |
| Translations lag behind the English text | mixed-language screens | the build reports missing keys per language; `check-locale.mjs` must pass for every language |

## Open questions

1. Which 1.x version is the oldest a hub may update from directly?
2. Does anything besides the 1.x web UI call API v1: Home Assistant, scripts,
   `smhub-broker`, other tools (D7)?
3. Which programs use `/run/smhub-backend.sock`, besides the tests (D42)?
4. Which languages need a native speaker's review before the stable release?
5. Continuous integration for pull requests on GitHub, and a hub in the loop
   for the hardware checks.
