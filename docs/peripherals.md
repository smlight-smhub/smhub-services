# The hub's own hardware

How `smhub-services` and `smhub-ui` 2.0 drive the hub's own hardware: the buzzer, the ambient
light ring, the service LEDs, the buttons, the infrared receiver and
transmitter, and a UPS board on J10. It says what each device does, how the
daemon and ESPHome share it, what each model has, the API, and where it appears
in the UI. The decisions behind it are D54 (one module per device, through whoever owns it),
D55 (API tokens) and D57 (the UPS board).

Facts about the hardware and the OS come from the development hub (SMHUB
Essential, hardware 1.01, OS 1.0.2, `esphome-bin` 2026.5.3-5, `smhub-broker`
1.0.5) unless marked otherwise.

## In plain words

The hub has two "brains". The main computer (Linux) runs the hub's web page,
the apps and the daemon, `smhubd`. A small second core runs ESPHome, which is
what Home Assistant connects to. A piece of hardware is driven by one of them
at a time.

The three switches on **Settings → ESPHome** choose which brain gets the light
ring, the buzzer and the I2C pins on J9. They take effect after a restart and
are on by default.

| Feature | Switch on (default) | Switch off, after a restart |
|---|---|---|
| Light ring | ESPHome drives the ring, and Home Assistant shows it as a light. The hub's page changes it too, by asking ESPHome: whoever changes it last wins, and both see the result | the hub drives the ring itself, LED by LED, with an effect speed. Home Assistant loses its light and reaches the ring through the hub's API |
| Buzzer | ESPHome plays the melodies, from Home Assistant and from the hub; a new melody replaces the one playing | the hub plays the melodies itself. Home Assistant loses the buzzer and reaches it through the hub's API |
| User I2C | the pins belong to ESPHome | the pins belong to Linux, for programs on the hub |

Without a switch:

| Feature | Who drives it | What the hub adds |
|---|---|---|
| Service LEDs | ESPHome while its firmware is installed (Home Assistant can switch them, and ESPHome keeps them over a restart); the hub itself without it | the page switches them; "all LEDs off"; night mode |
| BTN1 (full-size SMHUB) | nobody: the kernel reports each press, and ESPHome passes it on to Home Assistant | an action of the user's choice on a short, double or long press; Home Assistant still gets every press |
| BTN2 (every model) | the OS's reset program: held 10 s, it resets the hub to factory settings. ESPHome passes each press on to Home Assistant | the same as BTN1, with a long press of 1 to 3 s; the 10 s factory reset stays as it is |
| RST (full-size SMHUB) | restarts the chip at once; no software sees it | nothing |
| Infrared | only Linux has it: ESPHome has no infrared | learns buttons from remotes and sends them; Home Assistant sends them through the hub's API |
| 3.5 mm audio | nobody: the OS has no driver for it | nothing (see "Not supported") |

Updating a hub changes nothing by itself: each sound or light feature of the
hub is off until the user turns it on.

## How it is built

```text
 page (UI)   Home Assistant / scripts (API + token)   hub events   button actions
     \                 |                                   |            /
      +----------- smhubd: one module per device ---------------------+
      |   one API, queues, priorities, quiet hours, settings in the DB |
      +------------+-----------------------------------+--------------+
                   |                                   |
          ESPHome backend                        Linux backend
   (ESPHome's API; rtos-notify)   (PWM files, spidev, gpioset, key events, ir-ctl)
                   |                                   |
     second core drives the device         the main computer drives it
```

| Device | Backend | Decided by |
|---|---|---|
| Buzzer, light ring | ESPHome when its switch on Settings → ESPHome is on (the default); Linux when it is off | the switch, after a restart |
| Service LEDs | ESPHome while its firmware is installed; Linux without it | whether the firmware is there |
| Buttons, infrared | always Linux: the buttons are the kernel's `gpio-keys` on every model, and their key events can be read by any number of programs | — |

Both backends are full paths behind the same API and page, not a main path and
a fallback. Every shipped hub uses the ESPHome backend by default. What differs
is what the device can do: the API states each device's capabilities
(`per_led`, `speed` and `effects` for the ring, `can_stop` for the buzzer), and
the page shows only what works on this hub. With ESPHome that is what its
firmware offers (see "What ESPHome's firmware offers"); with Linux, everything
the daemon draws and plays itself.

### How the daemon finds the owner

The owner is read from the hub, never configured: at the daemon's start and
after every package change made through the hub (an app installed or removed).
The API does not let a caller choose it; `owner` in the `hardware` resource only
reports it.

| Situation | How the daemon sees it | Owner |
|---|---|---|
| ESPHome installed, the switches on | `/opt/firmware/smhub-rtos.elf` present; no `/sys/class/pwm/pwmchip0`, no `/dev/spidev0.0` (U-Boot gave them to the second core) | `esphome` for the buzzer, the ring and the LEDs |
| ESPHome installed but not answering (crashed, restarting, being updated) | the same files; the API connection is down | still `esphome`: commands fail with "ESPHome is not answering" (503), the pages say so, and nothing is taken over. A crash never moves a device, or the user's setup would change behind their back (D3). The buzzer tries `rtos-notify` instead |
| A switch off and the hub restarted | `pwmchip0` or `spidev0.0` present | `linux` for that device |
| No firmware (`esphome-bin` removed, or the file missing) | no `smhub-rtos.elf` | `linux` for the LEDs. The buzzer and the ring follow the overlays: they stay `none` until the ESPHome switches are off and the hub restarted. Never an automatic restart |
| One of the OS's C services runs and holds the PWM or SPI device | its OpenRC service is started | `other`: the hub leaves that device alone |
| The model has no such device | the model table (D27), the kernel's key list | `none` |

### The Linux backend

| Device | Linux backend | Cost while in use | Cost while idle |
|---|---|---|---|
| Service LEDs | one `gpioset` holds the lines `led_pwr` and `led_cus` at their values (the tool is in the image); a change ends it and starts a new one | one small process | the same, while the hub drives the LEDs |
| Buttons | the daemon reads the key events of the `gpio-keys` device (`/dev/input/event1` on the full-size SMHUB): plain reads of whole input events. `gpiomon` cannot take these lines ("Device or resource busy") | one open file and a blocked read | none |
| Buzzer | the daemon plays RTTTL itself through the PWM files (`/sys/class/pwm/pwmchip0/pwm2`), plain file writes, at a 50 % duty cycle | a timer per note, only while playing | none |
| Light ring | the daemon writes the 12 WS2812 LEDs' data to `/dev/spidev0.0` at 6.4 MHz; setting the bus speed needs an ioctl, so it uses the `spidev` crate (D13). The look is kept in `smhubd.db` (`hub_settings`, key `light_look`) and shown again at start | a steady colour costs nothing after it is written; an effect redraws 25 times a second on the main core | none |
| BTN2's factory reset | unchanged: the OS's own program | — | — |
| Infrared | always Linux (see "Infrared remote") | — | — |

### The OS's C services

The OS image carries `smhub-buzzer-daemon`, `smhub-ambilight-daemon` and
`smhub-mqtt-bridge`. Nothing starts them: none is in a runlevel, no other file
refers to them, and no other program links their libraries. The daemon does not use them: its own Linux backend needs no extra
service, has no idle wake-ups (the ring service wakes 18 times a second while
the ring is off) and follows the daemon's rules (no panics, limits everywhere),
with one API in front of both owners. Removing the services is a change to the
OS image, not to the `smhub-services` package. If one is started by hand, the
daemon leaves its device alone (owner `other`), so the two never drive the same
hardware.

## Principles

The rules of this repository, applied to hardware:

1. **One owner per device, and the hub never fights it** (D25). When ESPHome
   owns a device, the hub asks ESPHome. It never writes a pin, a PWM channel or
   the SPI bus that another program owns.
2. **Nothing changes on update** (D3). After updating to 2.0, every light, LED
   and sound behaves as before. Each hub feature that makes a sound or changes a
   light is off until the user turns it on.
3. **Home Assistant and the hub are both users.** With ESPHome as the owner,
   both send commands to the same ESPHome entity. The last command wins, and
   Home Assistant sees what the hub did because ESPHome reports it. Nobody
   locks anybody out.
4. **Nothing runs while unused** (priorities 2 and 3). A command starts a tool
   that exits (`rtos-notify`, `ir-ctl`), as the daemon does with `gpioget` and
   `nmcli`. A listener runs only while a feature that needs it is on
   ("Listen for remotes"), or costs nothing while idle (the buttons' key events,
   a blocked read). No polling.
5. **Every queue, rate and duration has a limit**, listed under "Limits" in
   [architecture.md](architecture.md): sounds per minute, infrared sends per
   second, stored codes, code length.
6. **No OS image change.** Everything uses tools already in the image; the
   crates `snow` and `spidev` are in [decisions.md](decisions.md). The one file
   the hub adds to the boot, the UPS board's overlay, comes with the
   `smhub-services` package.
7. **The model table decides what appears** (D27): `buzzer`, `ambient_light`
   and `ir` are in it (full-size SMHUB only). The buttons come from the kernel's
   key list. Nano and Nano MR show only the LEDs and BTN2.
8. **API first** (D7): live resources over the WebSocket (D9), two UI levels
   (D35), every text translated.
9. **Usable without our UI.** Every action has an API v2 call, and the
   Infrared remote page offers "Copy for Home Assistant" (a `rest_command`), as
   SLZB-OS does.

## Ownership and coexistence, device by device

| Device | ESPHome owns it (default) | Linux owns it (the switch off and a restart; for the LEDs, no firmware) | Used by both? |
|---|---|---|---|
| Buzzer | Home Assistant and the hub both play through ESPHome's `play_rtttl` service. One sound at a time: a new one replaces the one playing | the daemon plays it through PWM, with its queue and priorities. Home Assistant reaches it only through the hub | yes, by Home Assistant and the hub, through one owner. Never two owners |
| Light ring | both control the ESPHome light, and each sees what the other set (ESPHome reports it) | the daemon drives the ring through SPI, LED by LED; Home Assistant loses its light entity | yes, the same way |
| Service LEDs | both switch the ESPHome switches. ESPHome keeps their state over a restart | the daemon holds the two lines with `gpioset` and keeps their state in `smhubd.db` (`hub_settings`, key `leds_look`) | yes |
| BTN1, BTN2 | owned by neither: the kernel's `gpio-keys` holds them. `smhub-broker` passes each press to ESPHome for Home Assistant, `smhub-reset-daemon` watches BTN2 for its 10 s hold, and the daemon reads the same key events for its actions | the same | yes: Home Assistant and the hub can both react to a press; the factory reset is untouched |
| Infrared | ESPHome has no infrared | the kernel; the hub uses it directly. `lircd` stays off, so it never holds the devices | Home Assistant uses it through the hub |

ESPHome stays the default, so Home Assistant keeps its native entities and the
hub is one more user. The ESPHome page says what each switch does, for example
for the light ring: "On: ESPHome drives the light ring and Home Assistant sees
it. Off: the hub drives it itself after a restart, and Home Assistant no longer
sees it." The switches are stored where 1.x stores them (its `esphome` settings
page), so 1.x and 2.0 read the same values.

## One place for every caller

Many callers can want the same device at once: the page in two browsers, Home
Assistant through the API, the hub's own events, a button action. In the daemon
they all pass one module per device, so the same rules hold for everyone:

| Device | Rule |
|---|---|
| Buzzer | one sound at a time, from a queue of at most 16, with four priorities: alarm (Find my hub) > sounds a person started (the page, the API, a button action) > hub event sounds > feedback clicks. A higher priority interrupts a lower one; the others wait in order of priority. An event sound or a click waits at most 10 s, then it is dropped. The same sound is not played again within 1 s; at most 60 s of sound a minute, except alarms. Quiet hours and mute silence event sounds and clicks only |
| Light ring | layers: the user's look underneath, a hub signal on top. When the signal ends, the user's look returns |
| Service LEDs | the same idea for night mode and "all LEDs off": out, then back as they were |
| Infrared | one transmitter, so sends go out one after another (32 waiting, at most 5 a second); learning takes the receiver, and a second learn gets "busy" (409) |

What goes where:

| Kept in | What | Why |
|---|---|---|
| the database (`smhubd.db`) | the settings (`hub_settings`, key `hardware`: event sounds, quiet hours, mute, the user's melodies, signals, night mode, button actions, "Listen for remotes"), the ring's and the LEDs' state when the hub drives them, the saved infrared codes (`ir_codes`), the API tokens | it survives a restart and is part of a backup; it changes only when a person changes something, so it costs the flash nothing |
| memory | the queues, the current signal, the last 100 actions (what, by whom and with what result, for example the doorbell played by the token "Kitchen tablet") for the page | they change many times a minute; writing each one would wear the flash (priority 5) and is not needed after a restart |

A play request is answered with what happened: `playing`, `queued` with its
position, or `dropped` with the reason (`quiet`, `repeat`, `limit`, `full`).
Every other command answers with the `hardware` resource, which also shows the
queue and the signal to every open page at once.

With ESPHome as the owner, Home Assistant talks to ESPHome directly, outside
this queue. For the ring and the LEDs its command becomes the newest state,
which the hub shows. A melody Home Assistant plays is not seen by the hub. The
queue governs everything that goes through the hub.

## Models

The model table (D27) decides what exists, so a Nano never loads, shows or
spends anything on hardware it does not have:

| Model | Buzzer | Light ring | Infrared | 3.5 mm audio | BTN1 | BTN2 (factory reset) | RST | Service LEDs | UPS connector J10 |
|---|---|---|---|---|---|---|---|---|---|
| SMHUB Essential, Premium | yes | yes | yes | socket, no driver | yes | yes | yes | yes | yes |
| SMHUB Nano (P1, P7, P10, MG21, MG24, MG26) | — | — | — | — | — | yes | — (a 2-pin reset header J8 inside) | yes | — |
| SMHUB Nano MR (MR1–MR7, MRW) | — | — | — | — | — | yes | — (a button S2 inside the case) | yes | — |

The buttons on the page follow the kernel's key list, not the table: the
`gpio-keys` device names the keys each board has
(`/sys/devices/platform/gpio-keys/keys`). From the schematics (Nano MGx r0.98,
Nano MR r1.03): each Nano has one GPIO button, S1, on porta 1 (`btn_1`) with
the same circuit as the full-size hub's BTN2 (10 kΩ pull-up to 1.8 V, 100 Ω,
TVS); porta 2 (BTN1) is not connected. The OS image's device trees (`boot.itb`
on the kernel partition, one per model) give both Nanos one `gpio-keys` key,
"Factory Reset", `KEY_RESTART`. The page draws RST on the full-size SMHUB only.

On a Nano or Nano MR: no Buzzer page and no Infrared remote page; the Lights page
is only the LEDs and night mode; the hardware API answers 503 for the missing
devices (409 for the UPS calls); there are no hub signals, as they are shown on
the ring. The code is in the one daemon binary for every model, but nothing of
it runs where the hardware is missing.

## Status, API and use from outside

Every device has the same three things: its live status on the page, an API to
read and use it, and a way for other systems to use it.

| Device | Kind | Live status on the page | API (under `/api/v2`) | Other systems |
|---|---|---|---|---|
| Light ring | output | on/off, colour, brightness, effect, the signal showing | `GET /hardware` (`light`); `PATCH /hardware/light` | Home Assistant through ESPHome; others through the API |
| Service LEDs | output | on/off for each | `leds`; `PATCH /hardware/leds` `{"power", "blue"}` | the same |
| Buzzer | output | playing or idle, which sound, by whom, what waits | `buzzer`; `POST /hardware/buzzer/play` `{"sound"}` or `{"rtttl"}`; `POST /hardware/buzzer/stop` (`clear`, default true, also empties the queue) | the same |
| BTN1, BTN2 | input | the buttons drawn as on the case (BTN1, BTN2, RST), lit while held; the last gesture, its time and its hold time | `button`, `button2`: `pressed`, `last` with `gesture`, `at`, `held_ms` | Home Assistant through ESPHome; others by the live resource, or a button action that calls a web address |
| Find my hub | output | running or not | `find`; `POST /hardware/find` `{"on": true}` | the API |
| Infrared | both | learning, the last code received, the saved codes | `GET /ir` and the calls in "Infrared remote" | the API; received codes in the live `ir` resource |
| UPS board | — | whether it is marked as connected, its bus | `GET /ups`, `PATCH /ups`, `POST /ups/scan` | the API |

The settings of all of them are one object, `GET /settings/hardware`.
`PATCH /settings/hardware` merges a change into it, object by object, so
`{"sounds": {"mute": true}}` changes one value. A wrong value is refused with
its field (422, for example `sounds.quiet`). A device this hub lacks, or one whose owner does not answer,
gets 503 (`unavailable`); a feature its owner cannot do (single LEDs or an effect
speed through ESPHome) 501 (`not_supported`); a full infrared or ESPHome command
queue, or a second learn, 409.

A button is an input, so it has a state and a last gesture but no commands;
reading it every few seconds would miss short presses, which is why its changes
are pushed. The LEDs and the ring are outputs: their state can be read and set.

**Over the WebSocket** (D9): the page's one connection subscribes to the live
resource `hardware`. The hub answers with the current state of the light, the
LEDs, the buzzer and the buttons, then sends each change the moment it happens:
a colour set in Home Assistant, an LED switched, a button pressed or released,
the gesture once it is known. Infrared has its own resource, `ir`, and the UPS
board shows in `system` (`features.ups`). Another program can subscribe the same
way with a token.

The live status of the light and the LEDs comes from the hub's own ESPHome
connection while ESPHome owns them (see "Reading ESPHome"), and from the daemon
itself otherwise. The buttons and infrared are always the hub's own.

**API tokens** (D55): besides the browser's sign-in cookie, the API accepts a
long-lived token per integration, made and revoked on Settings → Sign-in and
accounts (`/tokens`, at most 32) and sent as `Authorization: Bearer <token>`.
Its value is shown once; the daemon keeps only its hash. A token acts as the
user who made it, but cannot manage tokens, users, sign-in or security, or open
the terminal (403). While the hub has no password, the API works without a
token.

## Reading ESPHome

Without ESPHome's state the hub could not show the ring's current colour, put
the ring back after a signal, or show an LED that Home Assistant switched. So
the daemon is a client of ESPHome's own API on port 6053, as Home Assistant is:
it connects to `127.0.0.1:6053`, where `smhub-broker` passes the connection to
ESPHome on the second core, lists the entities, subscribes to their states and
sends commands the same way. It is ESPHome's public, versioned protocol, and
commands and states share one channel.

- **Encryption:** Noise NNpsk0 (`snow`) with the API key the hub manages on the
  ESPHome page (the firmware's factory key while none is set); a hand-written
  encoder for the dozen protobuf messages used. A new key on the ESPHome page
  restarts the connection with it.
- **Version:** the hub says hello as `smhubd`, API 1.14. ESPHome 2026 signs a
  client in on hello; the older sign-in message is sent too, for older
  firmware, and ignored by newer.
- **Finding the devices:** by name, because servers for API 1.14 send no object
  ids: the light "AmbiLight" (else the first light), the switches "Power LED"
  and "Blue LED", and the service with one string argument whose name says
  "rtttl", "play" or "buzzer" (the firmware's `play_rtttl`). The firmware's
  "User Button" and "Factory Reset Button" are for Home Assistant; the hub reads
  the buttons from the kernel.
- **Keeping the connection:** the hub pings after 30 s of quiet (ESPHome itself
  pings a quiet client after 60 s) and calls the connection dead after 90 s of
  silence; it reconnects after 1, 2, 5, 10, 30 and then every 60 s (commands
  sent meanwhile are refused). It answers
  ESPHome's own ping and time requests. State changes are pushed; nothing is
  polled. At most 32 commands and 64 events wait.
- **Next to Home Assistant:** a second client does not disconnect the first
  (checked with two clients at once).
- **Firmware details the hub evens out:** four effects are called "Rainbow
  Effect", "Color Wipe Effect", "Scan Effect" and "Fireworks Effect"; the API
  names every effect the same whoever draws the ring ("Rainbow"). ESPHome drops
  a transition sent next to an effect, so the hub sends none; ESPHome fades a
  steady colour by its own default.
- **When the hub drives the ring:** ESPHome still lists its light, marked
  failed, and still reports it (the OS asks it for blue at the end of every
  start). The hub takes ESPHome's light reports only while ESPHome owns the
  ring.
- **While it does not answer:** the light and the LEDs are marked unknown
  (`known` false), the Buzzer and Lights pages say "ESPHome is not answering",
  and commands to them get 503. The buzzer then tries
  `rtos-notify buzzer <rtttl>`, which goes through `smhub-broker`'s socket.

`smhubd --esphome SECS` connects with the stored key, prints the entities and
states for that many seconds and exits; `--esphome-send` sends a command after
the listing.

## Infrared remote

It makes every infrared device in the room (TV, air conditioner, AV receiver,
fan, projector) controllable from the hub and from Home Assistant. Full-size
SMHUB only, and only when the kernel offers both devices (`/dev/lirc0` and
`/dev/lirc1`).

**What the user can do**

- **Learn:** press "Learn", point a remote at the hub, press a button. The page
  shows the code at once (live, over the WebSocket): its protocol and code when
  the kernel recognised it (for example "RC6_0 0x58"), otherwise "raw" with the
  number of timings. Repeat frames (a held button) are ignored.
- **Keep** it with a name and a device ("TV · Power", "Air conditioner · Cool
  22 °C"). Codes are grouped by device.
- **Send** a saved code with a tap; mark it as a favourite; rename or delete it.
- **Remote card on the home page:** the favourite codes as buttons, for example
  "TV · Power". It is one of the home page's cards on hubs with infrared, and can
  be hidden.
- **Copy for Home Assistant:** a `rest_command` for a saved code
  (`POST /api/v2/ir/codes/{id}/send`, with a token from Settings → Sign-in and
  accounts), ready to paste.
- **Advanced level:** "Listen for remotes", sending by protocol and code without
  learning (NEC, RC-5, RC-6, JVC, Sony, Sanyo, Sharp, RC-MM and their variants),
  and export and import of the code list as JSON. Raw timings and a carrier from
  20 to 60 kHz (default 38 kHz) can be sent through the API.

**How it works**

- Learning switches on the kernel's decoders on `rc0` (`lirc +nec +rc-5 +rc-6
  +jvc +sony +sanyo +rc-mm` into `/sys/class/rc/rc0/protocols`; a name without
  `+` would replace the whole set) and starts `ir-ctl -d /dev/lirc0 --receive
  --one-shot` for the raw timings and, next to it, `ir-keytable -s rc0 -t` for
  the protocol and code. `ir-keytable` writes to a pseudo-terminal: into a pipe
  it holds its output back, and the codes would come too late. When the kernel
  names no protocol, the daemon decodes NEC from the timings itself (`nec`,
  `necx`, `nec32`). A capture of fewer than 8 pulses is not a code (a lamp, the
  sun, or the hub's own sending) and learning waits on; it ends with the first
  real code, after 30 s, or with "Cancel" (`DELETE /ir/learn`), and both helpers
  end with it. Afterwards the receiver is back to raw only, unless "Listen for
  remotes" is on.
- A code is kept with its protocol and code when it has one, and with its raw
  timings when it comes from learning. The receiver does not measure the
  carrier, so a learned code is kept with 38 kHz.
- The hub cannot read its own sending: the kernel's sender driver
  (`gpio-ir-tx`) keeps interrupts off while it sends (79 ms for one NEC code), so
  the receiver sees one late pulse of about 230 µs per send. A self-test shows
  only that light leaves the hub; reading a code back needs a second device.
- Sending runs `ir-ctl -d /dev/lirc1`: `--scancode=<protocol>:0x…` when the code
  has a protocol (shorter, and repeats work as the device expects), otherwise
  `--send` with the raw timings and `--carrier`. Air conditioners send long codes
  the kernel does not decode; they are learned and sent raw, one code per state
  ("Cool 22 °C").
- Codes live in the daemon's own database, `smhubd.db`, so they are part of a
  backup. Nothing runs between commands.
- **"Listen for remotes"** (off by default): while on, `ir-keytable` runs in test
  mode and each decoded code becomes `last` in the `ir` resource and a line of
  recent activity; a code that matches a saved one (same protocol and code) is
  reported under its name.

**Limits:** 256 saved codes, an import included; a name of 1 to 40 characters,
a device name of at most 40; raw timings 1 to 1,024 values, starting and ending
with a pulse, each 1 µs to 100 ms, at most 2 s in all; one learn at a time,
30 s; 32 sends waiting, at most 5 a second.

**With ESPHome:** not involved. Home Assistant sends codes through the hub's
API.

**API:** `GET /ir`; `POST /ir/learn` (answers at once; the code arrives in `last`
of the `ir` resource) and `DELETE /ir/learn`; `POST /ir/codes` (`from_last`, or
a protocol and scancode, or raw timings), `PATCH` and `DELETE /ir/codes/{id}`;
`POST /ir/codes/{id}/send`; `POST /ir/send` with a protocol and scancode or raw
timings and a carrier; `GET /ir/export` (`{"format": "smhub-ir-1", "codes":
[…]}`) and `POST /ir/import`; `PATCH /settings/hardware` `{"ir": {"listen":
true}}` for "Listen for remotes".

## Buzzer

Full-size SMHUB only.

**What the user can do**

- **Sounds:** a list of presets with Play, and Stop while one plays. The presets
  use the same RTTTL melodies as SLZB-OS (Doorbell, Alert, Success, Error,
  Warning, Notification, Siren, Chime, Beep, Double beep, Startup, Shutdown), so
  the company's products sound alike.
- **Hub event sounds,** each a switch, all off by default:

  | Event | Sound | When |
  |---|---|---|
  | An update finished | Success | an OS update or an app operation (install, upgrade, removal, refreshing the app list) ended well |
  | Something failed | Error | one of those, a radio flash, a backup restore or a factory restore failed, or a backup download did not finish |
  | An app or service stopped by itself | Warning | a service is newly seen as crashed |
  | A backup finished | Notification | a backup download ended |
  | The hub started | Startup | once per boot, not when only the daemon restarts |
  | The hub is restarting or powering off | Shutdown | a restart or power off asked for through the hub |

- **Quiet hours:** no event sounds between two times of day (by default 22:00
  to 07:00), in the hub's time zone. A sound the user plays still plays.
- **Mute:** one switch that silences the event sounds and the buttons' feedback
  clicks. It does not change what Home Assistant plays through ESPHome.
- **Find my hub** (Settings → System): a chime every 2 s for 30 s, or until
  Stop, and an amber pulse on the ring. It plays in quiet hours and with mute;
  during night mode or "all LEDs off" the ring stays dark.
- **Advanced level:** a melody of the user's own (checked before it plays: the
  `name:defaults:notes` form, at most 512 characters, 256 notes and 30 s), tried
  at once or kept under a name next to the presets (at most 32 melodies, names
  of 1 to 40 characters); and the recent activity.

**How it works:** while ESPHome owns the buzzer, each sound goes to the
firmware's `play_rtttl` service as RTTTL text; while ESPHome does not answer, to
`rtos-notify buzzer "<rtttl>"`. A stop sends a melody that is one short rest,
which replaces the one playing. While Linux owns it, the daemon plays the notes
on the PWM output itself. The queue rules are in "One place for every caller".

**With ESPHome:** Home Assistant can play on the same buzzer through ESPHome's
`rtttl` at any time; the newest sound wins. The hub's quiet hours and mute apply
only to sounds the hub starts.

**API:** `POST /hardware/buzzer/play` with `sound` (a preset id or a saved
melody's name) or `rtttl`; `POST /hardware/buzzer/stop`; `sounds` in
`/settings/hardware` (`events`, `quiet`, `mute`, `melodies`).

## Ambient light

The ring of 12 WS2812 LEDs on the full-size SMHUB.

**What the user can do**

- **Light card:** on/off, colour (ten swatches and the browser's own colour
  picker), brightness, effect (the firmware's own list, read from ESPHome:
  Pulse, Strobe, Random, Rainbow, Color Wipe, Scan, Fireworks; the hub draws the
  same seven itself). A colour, brightness or effect turns the ring on. Changes
  apply at once; the card shows the live state, including a change made in Home
  Assistant a second ago.
- **Effect speed** (1 to 100) while the hub draws the ring. Through the API, the
  hub can also show a still picture of 12 colours (`pixels`), one per LED.
- **Hub signals,** off by default: the ring shows a hub state and then returns
  to what it showed before:

  | Signal | Look | When |
  |---|---|---|
  | Working | slow pulse, light blue | an OS update, an app operation, a radio flash, a backup or factory restore runs |
  | Done | green for 1.5 s (three flashes when the hub draws the ring) | it finished |
  | Failed | red for 1.5 s (three flashes when the hub draws the ring) | it failed |
  | Attention | amber pulse | "Find my hub", whether signals are on or not |

  Working and Attention show at the user's brightness, at least 50 %.
- The ring obeys the LED settings below ("all LEDs off", night mode); no signal
  shows while they keep it dark.

**With ESPHome:** Home Assistant and the hub both change the ring. A signal
remembers the look when it starts; if Home Assistant changes the ring during the
signal, its choice stays and the signal does not put the old look back. A change
from the page ends a signal the same way. The hub keeps no look of its own
for the ring; at the end of every start the OS sets it to blue at half
brightness (`status-login-ready`, through `rtos-notify`).

**API:** `PATCH /hardware/light` with `on`, `color` (`#rrggbb`), `brightness`
(0–100), `effect` (`None` or one of `effects`), and, while the hub draws the
ring, `speed` and `pixels` (12 colours; an empty list removes the picture);
`light.signals` in `/settings/hardware`.

## Service LEDs and night mode

Modelled on SLZB-OS's "LEDs settings", on every model:

- **Power LED** and **blue LED** switches, live.
- **All LEDs off:** the service LEDs and the ring stay dark until it is switched
  off again.
- **Night mode:** every light goes dark between two times of day (by default
  23:00 to 07:00), then back to how it was. It uses the hub's time zone and
  follows a change of the time zone at once.
- The blue LED stays the user's (Home Assistant automations use it). The hub
  never uses it for its own signals.

**With ESPHome:** ESPHome keeps the LED states over a restart, so the hub stores
nothing for them. During night mode, a change from Home Assistant wins: an LED
or the ring switched on in the night stays as it was set, and the end of the
night does not change it.

**API:** `PATCH /hardware/leds` with `power` and `blue`; `leds` in
`/settings/hardware` (`all_off`, `night`); `dark` in the `hardware` resource says
whether night mode or "all LEDs off" is in force.

## Buttons

Counted from the left on the case: **BTN1** (full-size SMHUB), **BTN2** (every
model; held 10 s it resets the hub to factory settings) and **RST** (restarts
the chip; no software sees it). The page draws them so, lit while held.

- **Where presses come from:** the kernel's `gpio-keys` key events, BTN1
  `KEY_SETUP` (141), BTN2 `KEY_RESTART` (408), read by the daemon on every hub
  with or without ESPHome. A blocked read: nothing runs between presses.
- **Gestures:** a short press (under 1 s), a double press (a second press
  within 400 ms) and a long press: held 1 s or more on BTN1, 1 to 3 s on BTN2. A
  short press is reported 400 ms after the release, once no second press came. A
  longer hold on BTN2 does nothing in the hub, because it may be on its way to
  the factory reset; `smhub-reset-daemon` keeps that job unchanged.
- **Each gesture can do one hub action,** chosen from a list, none by default,
  for each button: stop sounds and "Find my hub"; the light ring on or off; the
  power LED on or off (the blue LED goes off with it); play a sound (a preset or
  one of the user's melodies); send a saved infrared code; call a web address (a
  POST of `{"source": "smhub", "button": "btn1", "gesture": "short"}`, for
  example to a Home Assistant webhook, with a 5 s time limit). An action the
  model has no hardware for cannot be chosen.
- **Feedback,** optional, on models with a buzzer: a click of 38 ms when an
  action ran. It has the lowest priority in the buzzer's queue, and quiet hours
  and mute silence it.
- Every press and release is also a change of the `hardware` resource, and each
  action a line of recent activity.

**With ESPHome:** Home Assistant still receives every press of both buttons
("User Button", "Factory Reset Button"). The hub reacts only to gestures the
user gave an action to.

**API:** `button` and `button2` in `/settings/hardware`, each with `short`,
`double` and `long` (`action` and its `preset`, `code` or `url`) and `feedback`.

## UPS board

The UPS page is in development builds only: the Vite development server, or a
build with `VITE_UPS=1` (`ui/src/shell/routes.ts`). The release packages leave
it out, so a board is not marked there; the API below is in both.

An SMLIGHT UPS board plugs into J10 on the full-size SMHUB (D57). Nothing on the
board tells the hub reliably that it is there: J10 pin 5 (`ups_pwr`), which 1.x
reads as "UPS fitted" (`ups_check`), reads Low on a hub without one, because the
pad's own pull-down wins and Linux cannot switch on a pull-up; the development
hub, which has no UPS board, reads as having one. So the user says so: the
switch "A UPS board is connected" (Peripherals → UPS, `PATCH /ups`), saved in
`smhubd.db` (`hub_settings`, key `ups`), off by default. While it is on:

- the `system` resource reports `features.ups`;
- the hub talks to the board over I2C2 on J10 pins 1 (SCL) and 3 (SDA). The
  board's device tree muxes I2C2 (with pull-ups) but leaves it disabled, so the
  daemon's package ships an overlay, `ups-i2c2.dtbo`
  (`packaging/smhub-services/overlays/ups-i2c2.dts`), that turns it on and
  names it `i2c2`, so it is `/dev/i2c-2`. The daemon copies it to
  `/mnt/misc/overlays` when the switch goes on and removes it when it goes off;
  the OS's `dtbo-watcher` lists the folder in `uEnv.txt` at once and U-Boot
  applies it from the next start. At start the daemon puts the folder in line
  with the setting (after a restored backup, or an overlay removed by hand).
  Applied offline with `fdtoverlay` to the development hub's own device tree
  and its three ESPHome overlays, it gives I2C2 `okay` and the alias, and
  changes nothing else.

The page shows whether the bus is on or follows after a restart (a "restart
needed" notice, `restart_needed` in `GET /ups`), and at the advanced level scans
it (`POST /ups/scan`, 409 while the bus is not on). The hub does not read the
board itself (see "Not supported").

## Without the ESPHome firmware

Derived from the OS's boot scripts, not tried on a hub. When
`/opt/firmware/smhub-rtos.elf` is missing (or does not start), `remoteproc`
skips the second core, and `smhub-broker`, which needs it, does not start. U-Boot
still applies the overlays, so while the ESPHome switches are on, Linux has
given away the buzzer, the ring and the I2C pins, and nobody drives them.

| What | Effect |
|---|---|
| Home Assistant | the hub's ESPHome device goes offline (nothing on port 6053), and with it the Bluetooth proxy and every automation that uses the ring, buzzer, LEDs or buttons through ESPHome |
| Light ring | dark: no boot animation (`chase-blue` skips because SPI0 is marked for the second core), and the end-of-start blue fails quietly. The hub reports it as not available |
| Buzzer | silent; the reset button's beep fails quietly. The hub reports it as not available |
| Service LEDs | the hub drives them itself (power on, blue off, or as last set through the hub) |
| Buttons | BTN2's factory reset still works: it has its own Linux program. The hub's button actions work too: the hub reads the kernel's key events itself |
| Infrared, radios, Wi-Fi, Ethernet, LTE, apps, the web page, backups, updates | unaffected: all on the Linux side. Bluetooth on Linux still works; only ESPHome's proxy is gone |
| The hub's ESPHome page | still listed while the `esphome-bin` package is installed; the broker shows as stopped |

To use the hardware from Linux instead, the three ESPHome switches are turned
off and the hub restarted: Linux then owns the buzzer, the ring and the I2C
pins, the boot animation runs again, and the hub drives them as in "The Linux
backend".

## What ESPHome's firmware offers

Read from the firmware on the development hub (`smhub-rtos.elf`, symbols and
strings):

| Device | Through ESPHome | Not in the firmware | Driven from Linux instead |
|---|---|---|---|
| Light ring | on/off, one colour for the whole ring, brightness, smooth transitions, a flash, 7 built-in effects (Pulse, Random, Strobe, Scan, Color Wipe, Fireworks, Rainbow). Through `rtos-notify` only on/off, colour, brightness and effect | single LEDs (a progress ring, a clock, one segment per status), effect speed, direction or a second colour, new effects. The firmware has no per-LED effect (`addressable_lambda`) and no frame input (E1.31, WLED, Adalight) | effects with a speed and still pictures, LED by LED; animations cost CPU on the main core, and Home Assistant loses its light entity |
| Buzzer | any RTTTL melody: any notes, tempo and length, one voice. Home Assistant has a one-string service for it | a stop (the hub sends a short rest instead), volume, a queue or priorities, sounds RTTTL cannot write (sweeps, glides) | any tone and length through PWM; Home Assistant loses the buzzer |
| Service LEDs | on/off | blinking patterns (they are switches) | the same on/off |

So ESPHome limits the ring the most, the buzzer a little, the LEDs hardly. A
few additions to the firmware would lift most limits without taking the devices
away from it, for Home Assistant and the hub alike: a service that takes 12
colours and shows them through an `addressable_lambda` effect; speed and a
second colour for the effects, as `number` and `light` entities the effects
read; a stop for the buzzer. Animations would stay on the second core, and the
hub would draw by sending an occasional picture, not a stream of frames. The
daemon's API already carries the capabilities (`per_led`, `speed`, `can_stop`),
but uses single LEDs and speed only when it draws the ring itself.

`rtos-notify` accepts, from its usage text:
`light <id> <0|1> <brightness 0-1> <r> <g> <b> [effect]`, `switch <id> <0|1>`,
`buzzer <rtttl>`, `restart`. It sends commands only: it cannot read a state or
report an event.

## Not supported

- **The 3.5 mm audio output.** The OS has no driver for the SG2000's audio
  codec, so there is no sound card. Playing sound files or streams, and using
  them for hub events, needs an ALSA card for the output with a mixer for the
  volume first. ALSA and `bluez-alsa` (Bluetooth audio, a different path) are in
  the image.
- **Reading the UPS board:** battery level, power source and the like. The hub
  only marks the board, turns its bus on and scans it; reading it needs the
  board's I2C protocol.
- **Single LEDs, effect speed and a second colour while ESPHome drives the
  ring** (501), and a volume for the buzzer (see "What ESPHome's firmware
  offers").
- **Codes in protocols the kernel does not decode** (many Samsung, LG, Panasonic
  and air-conditioner codes) work in raw form only: reliable, but shown without
  a protocol name.
- **Button events over MQTT** and a Zigbee "permit join" action: a button
  reaches other systems through Home Assistant's ESPHome device, the live
  resource, or a web address it calls.

## Where it appears in the UI

| Page | Level | Contents |
|---|---|---|
| Peripherals → Buzzer | simple; own melodies and recent activity in advanced | presets, Stop, event sounds, quiet hours, mute; full-size SMHUB only |
| Peripherals → Lights | simple; recent activity and who drives each part in advanced | the ring (colour, brightness, effect, speed, hub signals), the LEDs, "all LEDs off", night mode |
| Peripherals → Infrared remote | simple; "Listen for remotes", protocol send, export and import in advanced | learn, devices and codes, send, favourites, the Home Assistant snippet; full-size SMHUB only |
| Peripherals → Buttons | simple | BTN1, BTN2 and RST drawn as on the case with their live state and last gesture; each button's gestures and actions; feedback |
| Peripherals → UPS | simple; the bus scan in advanced; development builds only (`VITE_UPS=1` puts it in a release build) | "A UPS board is connected", the I2C bus's state; full-size SMHUB only |
| Peripherals → USB | advanced | the USB port's mode |
| Home page | simple, a card that can be hidden | favourite infrared codes, on hubs with infrared |
| Settings → System | simple | Find my hub, on hubs with a buzzer or a ring |
| Settings → ESPHome | simple | the three ownership switches with clear wording, on hubs with ESPHome |
| Settings → Sign-in and accounts | simple | API tokens |

Peripherals is a section of the main menu, after Radios: the hub's own
hardware is part of what the hub does, not a setting. Pages for hardware a model
lacks are not listed; Lights and Buttons are on every model. While ESPHome drives
a part of a page and does not answer, the page says so.

## The OS programs involved

Measured on the development hub:

| Program | Language, size | Running? | Memory, CPU | What it does | Needed? |
|---|---|---|---|---|---|
| `smhub-broker` (package `smhub-broker` 1.0.5) | C++ (`smhub::hal`; nanopb, libgpiod, D-Bus, OpenSSL, Avahi), 735 KB | yes, 8 threads | 6.0 MiB resident, of which about 1 MiB its own (the rest is shared library code); 17.5 s of CPU in 6.2 h (0.08 %) | the second core's door to the outside: ESPHome's API server on port 6053 (encrypted) and OTA on 3232, forwarded over rpmsg; the Bluetooth proxy (the radio is on the Linux side); the LED and button pins for ESPHome; the mDNS announcement; the socket `rtos-notify` uses | yes: without it Home Assistant loses the hub's ESPHome device, the Bluetooth proxy, the LEDs and the buttons, and the hub loses its ESPHome connection |
| `smhub-reset-daemon` (same package) | 19 KB | yes | 1.2 MiB resident, 0.1 MiB its own | the reset button: factory reset after a 10 s hold | yes |
| `rtos-logger` (same package) | 14 KB | yes | 2.7 MiB resident, 0.2 MiB its own | writes the second core's log on Linux | for diagnosis |
| `rtos-notify`, `smhub-arbitration`, `smhub-update-overlays` (same package) | 45 KB, 14 KB, shell | only when called | — | commands to ESPHome; device ownership; the overlay list for U-Boot | yes, as tools |
| `smhub-buzzer-daemon`, `smhub-ambilight-daemon`, `smhub-mqtt-bridge` (OS image) | C; binaries 18–35 KB, about 150 KB with their tools and shared library | no | nothing while stopped. Started by hand: the ring service 1.6 MiB resident, 0.2 MiB its own, 3 threads, and while the ring is off it wakes 18 times a second (0.08 % CPU); the MQTT bridge 4.2 MiB resident, 0.8 MiB its own, 2 threads, no CPU; the buzzer service exits at once ("failed to initialize PWM chip0/pwm2") because ESPHome owns the output | the same buzzer and ring, driven from Linux, and an MQTT front for them | no |

`smhub-ambilight-daemon` and `smhub-ambilightctl` come with a config and an init
script, but could not run on a default hub anyway: with ESPHome owning SPI0,
Linux has no `/dev/spidev0.0`.

The daemon uses `smhub-broker` as it is. It is lean (about 1 MiB of its own
memory, 0.08 % CPU), and its parts are the ones with the most risk: the ESPHome
API with its encryption, the Bluetooth proxy and the link to the second core.
Its source and that of `rtos-notify` are not part of this repository, and the
socket protocol between them is not described.

## Checked on hardware

On the development hub, with ESPHome 2026.5.3 on the second core:

| Check | Result |
|---|---|
| `tests/hub/hardware_test.py` (`--sound` for the buzzer) | the settings merge, refusals and put-back; tokens (read, 403 on token management, 401 for a wrong or revoked token); the ring's colour, Pulse and Rainbow reported back, a brightness change while off turns it on, the look put back; the blue LED; an infrared code saved, sent, renamed, exported and deleted, raw timings sent; a beep played and a repeat dropped |
| The ESPHome connection | connected in 140 ms; six entities by name (AmbiLight with 7 effects, Power LED, Blue LED, User Button, Factory Reset Button, the `play_rtttl` service), the states at once; two clients at once, neither disconnects the other |
| The ring driven by the hub (the light switch off, the hub restarted) | the light's owner `linux`, the other devices still `esphome`; single LEDs in the right colour order; the seven effects; the look back after the daemon's restart |
| CPU while the hub draws the ring | an effect 0.7–0.8 % of a core (25 frames a second), each over 20 s from `/proc`; a steady colour is written once, then 0 clock ticks in 20 s |
| The daemon with the ESPHome connection open, every hardware task idle | 2 threads, about 1.2 MiB of its own memory, no CPU time in 60 s |
| Binary size | the encryption for ESPHome's API (`snow`, X25519, ChaCha20-Poly1305, SHA-256) is 71 KB of the daemon's code, by symbol name in an unstripped build |
| Infrared learned from a Philips TV remote | `rc6_0`, code `0x58`, 39 pulses; afterwards the decoders off and no helper left |
| Infrared sent and received by the same hub | one late pulse of about 230 µs per send (see "Infrared remote") |
| BTN1 and BTN2 pressed | short presses reported about 0.4 s after the release, double and long presses as set; BTN2 held 6.5 s: nothing in the hub, and `smhub-reset-daemon` did nothing either; a short press set to the light ring turned it on and off |
| The UPS overlay | applied offline with `fdtoverlay`, as in "UPS board"; marking and unmarking the board puts the overlay in and out of `uEnv.txt` |

Not checked on hardware: a learned code sent to the device it came from; the
LEDs and the buzzer driven by the hub itself; the sounds heard by a person; the
button actions that play a sound, send an infrared code or call an address; the
UPS bus coming up after a restart with the overlay; a hub without the ESPHome
firmware.

## Open questions

1. Can `smhub-broker`'s socket stop a melody or report a state? Without a
   description of it, the hub uses only the commands in `rtos-notify`'s usage
   text.
2. Will the ESPHome firmware (`esphome-bin`) get a 12-colour picture service,
   effect speed and a second colour, and a buzzer stop?
3. Will the OS get a driver for the 3.5 mm audio output?
4. Which chip and I2C protocol does the UPS board use (address, registers for
   battery level and power source)? The hub could then show them on the UPS
   page.
