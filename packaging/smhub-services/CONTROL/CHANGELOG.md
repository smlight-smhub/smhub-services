## v2.0.3-1
_released on 20261122_

* New core daemon smhubd, written in Rust: starts in under a second and uses a few MiB of memory
* The hub's own hardware: buzzer, light ring and LEDs, night mode, infrared remote and buttons
* New API v2 with live updates over one event connection; API v1 is gone except the readiness check
* Same database, app settings, backups, updates and factory reset as 1.x
* Radio flashing keeps the vendor flashers in a helper that runs only while flashing
* The services that keep the hub reachable (network, SSH, web server) cannot be stopped from the services list; they and Tailscale stay up during a backup

## v1.1.0-1
_TL released on 20260713_

* Support app provided icons
* rebuilds for buildroot 2026.05.x

## v1.0.8-1
_TL released on 20260713_

* Support multi-line config options

## v1.0.7-1
_TL released on 20260713_

* Fix futher process mapping for openrc app status

## v1.0.6-1
_TL released on 20260712_

* Fix setting esphome encryption key on Nano devices
* Fix syncing app autostart status on openrc 

## v1.0.5-1
_TL released on 20260710_

* Add support for configuring ser2net service controls
* Fix openthread app running status
* Fix bug when setting timezone from the web ui


## v1.0.4-1
_TL released on 20260606_

* Fix error on saving mosquito MQTT settings
* Handle combined update channel settings, os and app channels will always remain in sync
* Sync rtos settings on startup
* Fixes for log history and log export
* Fix invalid ieee error when flashing cc2652

## v1.0.3-1
_TL released on 20260529_

* Ensure correct Risc-V uv binary installed into venv
* fix service detection when flashing devices from radio page
* Provide Openthread status data for frontend

## v1.0.2-1
_TL released on 20260527_

* Add iframe proxy for dynamic port configuration
* resolve user backup/restore missing xz module

## v1.0.1-1
_TL released on 20260520_

* Fix error syncing config source for apps


## v1.0.0-1
_TL released on 20260519_

* Rename references to smhub-web package, now smhub-ui
* Dynamic config settings now stored in schema.json files
* Update default db to latest schema
* provide tailscale integration
* List dev firmware for radio updates
* Show-in-sidebar db setting
* Fix postinst when upgrade changes python ABI

## v0.2.13-1
_TL released on 20260320_

* Add Network Manager connection events (websocket)
* Add support for setting IEEE when flashing radios
* Fix bug in EFR radio flasher
* Add backend for File Manager
* Enable flashing Zwave radio
* Improve fetching Apps status data
* Bump universal-silabs-flasher v1.0.2
* Migrate venv to Python 3.14; require `python3-abi-3.14` virtual package (provided by OS 0.9.9+) to prevent installation on incompatible OS versions
* Defer postinst migration and service restart when installed during OTA pre-reboot window (Python 3.14 binary not yet active)

## v0.2.12-1
_TL released on 20260316_

* Disable AT Echo on modem startup
* isRunning API to check openrc for service status

## v0.2.11-1
_TL released on 20260219_

* Add schema fields to enable backup of user data
* Provide support to backup and restore user data
* Support Factory reset by pressing button for 10s

## v0.2.10-1
_TL released on 20260217_

* Update default db file
* Provide full support for services in OpenRC
* Support optional watchdog setting for apps

## v0.2.9-1
_TL released on 20260204_

* Provide service file for OpenRC
* Bump smlight_cc_flasher v0.1.6 for IEEE writing
* Add backend support for App watchdog

## v0.2.8-1
_TL released on 20260122_

* Migrate autostart files to new location
* Bump system-config v0.1.16
* Support configuration for OTBR app

## v0.2.7-1
_TL released on 20260118_

* Use CC flash reset for NanoMR4 EFR
* Optimise load times by deferring some components load
* Update gpio mapping for 6.18 kernel
* Force z2m to default to info logging

## v0.2.6-1
_TL released on 20251224_

* Add support for NanoMR4
* Add support for flashing CC2674P10

## v0.2.5-1
_TL released on 20251212_

* Fix run/stop buttons for Openthread app
* Fix default port for openthread on Nano

## v0.2.4-1
_TL released on 20251211_

* Add support for SMHUB Premium radio flashing
* Remove obsolete sniffio dependency
* Add additional z2m settings

## v0.2.2-2
_TL released on 20251125_

* Fix issue with default database contents

## v0.2.2-1
_TL released on 20251125_

* Add suport for reading v1 format EEPROM
* Update radio endpoints to support flashing models based on EEPROM
* Bump pysmlight v0.2.11 for Nano flashing
* Correct some default settings for zigbee2mqtt

## v0.2.1-1
_TL released on 20251121_

* Add smlight-cc-flasher and integrate flashing support
* Fix Zigbee2MQTT default settings in web form
* Fix logs for Node-red

## v0.2.0-1
_TL released on 20251115_

* Add pysmlight
* Add universal-silabs-flasher
* Add support for flashing EFR32
* Add required-OS metadata to packages

## v0.1.20-1
_TL released on 20251112_

* Prepare for Openthread App
* Activate dev channel for OS releases

## v0.1.19-1
_TL released on 20251105_

* Add endpoint to read eeprom data
* Add api to query and set timezone
* Fix bugs related to update version comparison

## v0.1.18-1
_TL released on 20251104_

* Add support for gpio control
* Add support for modem control

## v0.1.17-1
_TL released on 20251028_

* Update Network Manager support for Wifi and Ethernet
* Add setings for device hostname
* Wireguard fixes

## v0.1.16-1
_TL released on 20251022_

* Improvements to prerm/postinst scripts
* Update smhub-backend for Network Manager/Wireguard
* Add poweroff command

## v0.1.14
_TL released on 20251020_

* system-config v0.1.2 - Run opkg in detached session and log output

## v0.1.13-1
_TL released on 20251019_

* Fix validation error on apps setting pages

## v0.1.12-1
_TL released on 20251019_

* Add backend framework for settings pages
* Run database migrations on upgrade (if required)
* Bump system-config v0.1.1
* Move binary to /opt/bin/
* Fix permissions for mosquitto password file

## v0.1.9-2
_TL released on 20251014_

* Fix occasional crash in terminal backend

## v0.1.9-2
_TL released on 20251013_

* Login to web terminal as user smlight
* Ensure colour terminal

## v0.1.8-1
_TL released on 20251013_

* Add backend for web terminal
* Fix application logs for web and zwave apps
* Fix installation of smhub-services hanging from web ui
* Fix 401 error on HEAD requests filling logs
* Add endpoint for opkg update

## v0.1.7-1
_TL released on 20251012_

* Provide backend support for application logs
* Implement Pseudo-Terminal class for web console
* Implement Websocket interface for web console

## v0.1.6-1
_TL released on 20251010_

* Add support for OS upgrades using Rauc

## v0.1.5-1
_TL released on 20251010_

* Add missing schema

## v0.1.5-1
_TL released on 20251005_

* Add api endpoint for getting OS Version
``
## v0.1.3-1
_TL released on 20250922_

* Fixes for auto-login
* Add CORS origin for https

## v0.1.0.dev35-1
_TL released on 20250914_

* Fix CORS issues

## v0.1.0.dev35-1
_TL released on 20250712_

* new API's
    * detect if app is running
    * detect if app is active on url

## v0.1.0.dev25-3
_TL released on 20250710_

* (re)start service on upgrade

## v0.1.0.dev25-2
_TL released on 20250710_

* Fix permissions installing apps

## v0.1.0.dev18-1
_TL released on 20250707_

* Fix Packages.gz error on start
* Fix error on stop

## v0.1.0.dev14-5
_TL released on 20250707_

* Initial release packaged backend