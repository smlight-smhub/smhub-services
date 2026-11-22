## v2.0.3-1
_released on 20261122_

* New web UI: Simple and Advanced levels, live data, 31 languages with local formats for numbers, units and dates, light and dark themes
* Pages for the hub's own hardware: buzzer, light ring and LEDs, night mode, infrared remote and buttons
* Small and fast: about 230 KB to open the first page, works on phones and computers
* Reloads itself when the hub has a newer version installed, even from a browser cache

## v1.0.5-1
_TL released on 20260713_

* Support multi line topics in mosquitto mqtt

## v1.0.4-1
_TL released on 20260710_

* Add settings page to configure ser2net and control service
* Check USB mode before enabling 4G modem. Switches to host mode if required
* Show reboot notification after OTA update reboot
* Keep track of OTA progress even when navigating away from updates page

## v1.0.3-1
_TL released on 20260530_

* Add settings page for ESPHome
* Add modal dialog to restart after save app settings
* Various UI style improvements and polish

## v1.0.2-1
_TL released on 20260530_

* Show direct links for apps with iframe
* add description to picoclaw app controls
* Zigbee2MQTT theme, follows smhub setting dark theme
* Various UI style improvements and polish

## v1.0.1-1
_TL released on 20260527_

* Fix and polish dark theme styling across the entire UI, which was previously broken.
* Fix rendering of app control toggles
* Use backend iframe proxy for port allocation
* Fix rendering of OS update markdown changelogs
* Fix filtering of pre-release OS updates display
* Add desciption field to settings forms

## v1.0.0-1
_TL released on 20260519_

* Rename package from smhub-web to smhub-ui
* Add some missing translations for Apps settings forms
* Add advanced mode toggle on general settings page
* Introduce schema driven Status Card and Actions
* Add frontend integration for tailscale app
* Add show-in-sidebar option for Apps with iframe

## v0.3.2-1
__TL released on 20260319_

* Dynamically show installed Apps in Sidebar menu
* Allow setting IEEE Address when flashing Zigbee firmware
* Enable flashed on Zwave radio
* Integrate file manager

## v0.3.1-1
__TL released on 20260314_

* Add Reset Config button to apps (zigbee2mqtt)
* Sort apps on apps overview page
* Add Restart button on App Controls page


## v0.3.0-1
__TL released on 20260214_

* Ensure proper handling when sorting debian style versions
* Add Backups page for user data backup/restore (requires OS 0.9.6)
* Notify reboot required after OTA in status widget

## v0.2.25-1
__TL released on 20260214_

* Support zigbee2mqtt watchdog
* Fix clock to display in set timezone
* Colour theme improvements

## v0.2.24-1
__TL released on 20260128__

* Fix bug when saving z2m settings
* Fix sidebar hamburg button

## v0.2.23-1
__TL released on 20260101__

* Increase timeout while waiting for flasher to load

## v0.2.22-1
__TL released on 20260101__

* Temporarily disable ethernet static IP setting until bug is fixed

## v0.2.21-1
__TL released on 20251225__

* Update radio page for nanomr devices

## v0.2.20-1
__TL released on 20251220__

* Fix iframe app status when app not running

## v0.2.19-1
__TL released on 20251220__

* Save radio mode and version after flashing
* Fix iFrame loading after using restart app button
* Display settings pages based on device model
* Add update status widget for available os + apps
* Adjust time estimation for smhub-services update

## v0.2.18-1
__TL released on 20251212__

* Fix mosquitto settings pages and bridge mode
* Fix saving numbers in app settings
* Improve app settings config forms

## v0.2.16-1
__TL released on 20251124__

* Update radio page to support flashing NanoMg24
* Display model in sidebar
* Update some default settings for z2m. 
* Display app status on apps settings page
* Progress bar for OS updates
* Add changelogs for OS updates

## v0.2.15-1
__TL released on 20251120__

* Enable flashing CC radio
* Improved handling of progress bar and error messages on radio flashers
* Iframe improvements and added App Logs and Restart Buttons
* App control fixes for Run/Stop buttons
* Fix for logs that were rendered in "Title Case"

## v0.2.14-1
__TL released on 20251114__

* Add suport for flashing EFR32 radio
* Add warning on apps page if OS update is available
* Block individual apps upgrade if OS metadata set

## v0.2.13-1
__TL released on 20251110__
* Fix handling of OS dev channel updates
* Fix channel field in z2m config
* Fix autostart and logs of openthread app


## v0.2.12-1
__TL released on 20251107__

* Add support for changing Timezone
* Display if zwave module is installed
* Bug fixes for status on LTE page

## v0.2.11-1
__TL released on 20251104__

* Add Ethernet page
* Add Lte page for 4g internet

## v0.2.10-1
__TL released on 20251030__

* Add Wifi Page to configure connections
* Add USB page for usb mode and list devices
* Set hostname on General Page
* Wireguard now suppors hostnames/ipv4 and ipv6

## v0.2.9-1
__TL released on 20251022__

* Add Wireguard VPN Client
* Add reboot button to sidebar

## v0.2.8-1
__TL released on 20251019__

* Remove interface setting

## v0.2.6-1
__TL released on 20251019__

* Add settings page for Mosquitto MQTT
* Load settings from database (we need this to properly handle rewriting changed settings to config file)

## v0.2.5-1
__TL released on 20251013__

* Add Web Terminal

## v0.2.4-1
__TL released on 20251012__

* Support application logs
* Refresh button to update available packages

## v0.2.0-1
__TL released on 20251011__

* Support System OS Upgrades

## v0.1.9-1
__TL released on 20251008__

* Fix firmware version in sidebar

## v0.1.7-1
__TL released on 20250925__

* Fix iframe loading
* Proxy iframe content over https

## v0.1.6-1
__TL released on 20250925__

* Allow access over https://smhub.local
* Fix access over http://<IP>
* Remove service file and use nginx instead

## v0.1.3-1
__TL released on 20250922__

* Enable autologin
* Authentication Settings Page 
* Fix websockets with autologin

## v0.1.0-9
__TL released on 20250911__

* Autologin frontend

## v0.1.0-8
__TL released on 20250911__

* Various fixes

## v0.1.0-7
__TL released on 20250707__

* Fixes for apps install, uninstall and run

## v0.1.0-5
__TL released on 20250707__

* Add schema file

## v0.1.0-4
__TL released on 20250707__

* Bundle SMHUB frontend as package
