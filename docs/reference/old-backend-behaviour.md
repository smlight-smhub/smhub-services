# Old backend behaviour (Python `smhub-services`) - implementation reference

What the 1.x backend (the Python `smhub-services`: FastAPI and `system-config`) stores in its SQLite database, writes to the OS and runs as commands, read from its source code. 2.0 must stay compatible with it: an existing user's setup keeps working after the update to 2.0 and after going back to 1.x, so the database keeps its structure and meaning, and the OS files below keep their formats, except where [decisions.md](../decisions.md) records a deliberate difference (D58). The HTTP API and the UI are free to change; the old API is listed only so that you can find the behaviour behind each endpoint. The short list of fixed formats is in [architecture.md](../architecture.md#compatibility-with-existing-hubs). Code comments in `crates/` cite this document by section (`reference §8`).

## 0. Conventions and sources

Path abbreviations used in references (`file:line`):

| Abbrev. | Path in SMLIGHT's 1.x repositories | Revision read |
|---|---|---|
| `BE/` | `fastapi-backend/backend/smhub_backend/` | `7f3f5b7` (master) |
| `RS/` | `fastapi-backend/backend/src/` (Rust helper `smhub_rs`, PyO3) | same |
| `SC/` | `system-config/system_config/` | `e14145a` (main) |
| `PKG/` | `sys-builder/buildroot-external/package/smhub-services/` | `d8c18f8` (main) |
| `OS/` | `uhah-buildroot/buildroot-external/board/smhub-6/overlay/` (OS image files that interact with the backend) | `5d838e7` |
| `FE/` | `frontend/src/` (old React UI; only used to learn what the UI sends and therefore what is stored in the DB) | `a2003a8` |
| `APPS/` | `app-builder/buildroot-external/package/` (real app `schema.json` files) | `ab8700a` |

Marks:
- **Inferred** = not directly visible in the code read; deduced from library behaviour, other repos, or the absence of code. Check before relying on it.
- "UI" = the old React frontend. "Today" = the behaviour of the Python code at the revisions above.
- Secrets are never reproduced. Where a secret exists, the document names the file, line and kind.

Runtime facts that apply everywhere:
- The process is started by OpenRC as **root** (no `command_user` in `PKG/CONTROL/openrc:1-39`; **Inferred** root). Files it creates are therefore owned by root unless the code changes ownership.
- Listens on TCP `0.0.0.0:8000` (`BE/cli.py:6-7`) and on the Unix socket `/run/smhub-backend.sock` (`BE/main.py:312-334`). nginx on port 80 serves the UI and proxies `/api/` and `/ws/` to `127.0.0.1:8000` (`OS/etc/nginx/nginx.conf:17-47`).
- API prefix `/api/v1` (`BE/core/config.py:29`, `BE/main.py:402`); WebSockets are mounted without prefix at `/ws/...` (`BE/main.py:403`, `BE/api/routes/websocket.py:39`).
- `ENVIRONMENT=production` in the shipped `.env` (`BE/.env`), so `/docs`, `/redoc`, `/openapi.json` are disabled (`BE/main.py:370-377`) and `/api/v1/dev/reset-db` returns 404 (`BE/api/routes/dev.py:19-20`).
- CORS middleware is always installed (FRONTEND_HOST is always in `all_cors_origins`, `BE/core/config.py:40-45`) with a fixed origin list: `http://smhub.local`, `http://smhub.local:3000`, `https://smhub.local`, `http://<hostname>`, `http://<hostname>.local`, `http://<hostname>.local:3000`, `http://localhost:3000`, credentials allowed, all methods and headers (`BE/main.py:379-398`). The env value `BACKEND_CORS_ORIGINS` is parsed but not used for the list.
- Environment variables exported by the init script: `PATH=/opt/bin:$PATH`, `SMHUB_SERVICES_DATA=/opt/smhub-services`, `PKG_PATH=/var/lib/opkg/info` (`PKG/CONTROL/openrc:18-20`). The terminal inherits them (section 10).
- Other env switches read by the code: `SMHUB_MOCK_EEPROM_MODEL` (`BE/main.py:46`), `MOCK_TAILSCALE`, `MOCK_OPENTHREAD`, `OTBR_REST_API_URL` (default `http://localhost:8081`), `PYTEST_CURRENT_TEST` (disables the UDS server), `DEV_BUILD` and `MOCK_DEV` in system-config (switch paths to dev mocks). None of them is set on a device.

External OS components that depend on the backend (they are not part of the backend but break if it changes):

| OS component | What it relies on | Reference |
|---|---|---|
| `api-ready` service (`/usr/lib/smhub/api-ready-helper.sh`) | Polls `GET http://localhost:8000/api/v1/login/user-session` every 15 s, up to 10 times, `wget --spider -T 5`; status **200 or 401** = backend ready; then drives the boot LEDs and signals OpenRC ready. Other services wait for `api-ready`. | `OS/usr/lib/smhub/api-ready-helper.sh:3-71`, `OS/etc/init.d/api-ready:1-19`, `OS/etc/init.d/firstboot-upgrade:133-145` |
| `firstboot-upgrade` | Starts `smhub-services` with `rc-service --nodeps smhub-services start`, then starts every service that has a `/opt/share/services/<svc>.autostart` flag after `api-ready` is started. | `OS/etc/init.d/firstboot-upgrade:91-145` |
| `modem-power` | Powers the LTE modem at boot only if `/opt/share/services/lte.autostart` exists and `gpioget 4g_check` is 0. Runs `before smhub-services`. | `OS/etc/init.d/modem-power:1-46` |
| `zigbee2mqtt` OpenRC script (app package) | Reads `/opt/share/services/zigbee2mqtt.watchdog`. | `APPS/zigbee2mqtt/CONTROL/openrc:34` |
| `bluetoothd` | Reads `/opt/firmware/bluetooth_proxy_mode` (value `hci` changes behaviour). | `OS/etc/init.d/bluetoothd:23` |
| `remoteproc`, `dtbo-watcher`, `firstboot-production` | Use `/opt/firmware/smhub-rtos.elf` and `/mnt/misc/overlays/*.dtbo`. | `OS/etc/init.d/remoteproc:12`, `OS/etc/init.d/dtbo-watcher:3`, `OS/etc/init.d/firstboot-production:111-113` |
| `usb-watcher` / `usb-mode-trigger.sh` | Watches `/etc/usb_mode` (inotify) and switches the USB role. | `OS/etc/init.d/usb-watcher:7`, `OS/usr/lib/smhub/usb-mode-trigger.sh:6-13` |
| `sdcard-config` | Reads the flag `/mnt/user/.sdcard-config-enabled`. | `OS/etc/init.d/sdcard-config:16` |
| `set_default_zigbee_z2m.sh` | Edits `appsettings` directly with `sqlite3` (see 1.6). Called from the package postinst only on non-OpenRC systems. | `OS/usr/lib/smhub/set_default_zigbee_z2m.sh:1-60`, `PKG/CONTROL/postinst:78-89` |
| stunnel | `/etc/stunnel/stunnel.conf` has `include = /opt/smhub-services/config/stunnel.conf.d`. | `OS/etc/stunnel/stunnel.conf:1-8` |
| factory reset | Keeps `/mnt/user/.factory-backup` when wiping `/mnt/user`. | `OS/usr/lib/smhub/factory-reset-handler.sh:16` |

---

## 1. Database

### 1.1 File, engine, pragmas

- Path: `/opt/smhub-services/config/backend.db`, hard-coded (`BE/core/config.py:54-59`). Engine `create_engine("sqlite:////opt/smhub-services/config/backend.db")` with no options (`BE/core/db.py:28`).
- No `PRAGMA` is set at runtime: journal mode is SQLite's default (rollback journal) and **foreign keys are not enforced** (**Inferred**: SQLite default is off and SQLAlchemy does not turn it on). Several code paths depend on that (orphan `configsetting.source_name`, see 1.5).
- The only place that touches pragmas is the local-only `/dev/reset-db` (`BE/api/routes/dev.py:24-27`).
- Backups and restores call `engine.dispose()` to close pooled connections (`BE/api/routes/stream.py:153`, `:300`).
- First install copies the seed DB `/var/lib/opkg/info/smhub-services.db` (built from `PKG/CONTROL/db`) to the path and `chown -R smlight:smlight /opt/smhub-services/config` (`PKG/CONTROL/postinst:42-47`). The DB file is not part of the package file list, so it survives upgrades (**Inferred** from opkg semantics).

### 1.2 Value encodings (read this before touching any row)

| Kind | On disk | Source |
|---|---|---|
| `BOOLEAN` columns | INTEGER `0` / `1` | SQLAlchemy Boolean on SQLite; seed rows confirm |
| `DATETIME` columns | TEXT `YYYY-MM-DD HH:MM:SS.ffffff` (6-digit microseconds), UTC, **no timezone suffix** | `utcnow()` is tz-aware (`BE/models.py:129-130`) but SQLAlchemy's SQLite DATETIME drops the offset (**Inferred**); seed values look like `2025-06-02 08:43:14.726627` |
| `configsource.params` (JSON) | TEXT, `json.dumps` default style, e.g. `{"comment_prefixes": ["#"]}`; Python `None` is written as the text `null` (**Inferred**: SQLAlchemy JSON with `none_as_null=False`) | `BE/models.py:409` |
| `user.id`, `usersettings.id`, `fileitem.id` | TEXT UUID v4, lowercase with dashes (`str(uuid.uuid4())`) | `BE/models.py:48`, `:354` |
| `appsettings.value`, `devicesettings.value` | TEXT, but the textual form depends on the writer (table below) | |

How each writer turns a value into the `value` TEXT:

| Writer | `True` | `False` | `1883` | `["a","b"]` | `None` / JSON null | Reference |
|---|---|---|---|---|---|---|
| `POST /pages/{key}/settings` (`crud.update_page_settings`) | `True` | `False` | `1883` | `["a", "b"]` (`json.dumps`, `", "` separators, non-ASCII escaped as `\uXXXX`) | `None` (the 4-letter string) | `BE/crud.py:402-420` |
| `PUT /pages/{key}/settings/{k}` (body `{"value": "<string>"}`, must be a string) | as sent | as sent | as sent | as sent | rejected | `BE/api/routes/pages.py:237-272` |
| `POST /smhub/packages/{pkg}/settings` (`crud.update_app_settings`) | `True` | `False` | `1883` | `['a', 'b']` (Python `repr`!) | `None` | `BE/crud.py:196-206` |
| Schema defaults at startup (`sync_app_settings`, raw JSON value bound to a VARCHAR column; SQLite TEXT affinity converts) | `1` | `0` | `1883` | bind error (**Inferred**; no real schema has list defaults today) | n/a (missing default = empty string `""`) | `BE/core/db.py:192-198`; affinity verified with sqlite3 |
| `sync_pages_autostart` (startup) | `true` | `false` | | | | `BE/core/db.py:303-311` |
| Migrations / seed DB | `true` | | | | | `BE/alembic/versions/4cc919e10054_preseed_rtos_default_settings.py:48-54` |
| UI security page | sends the strings `"true"` / `"false"` itself | | | | | `FE/pages/Settings/AuthorizationSettings.jsx:76-79` |

The `isinstance(value, bool)` branches in `upsert_app_setting` / `upsert_device_setting` (`BE/crud.py:166-167`, `:372-373`) are dead code for HTTP writes, because the callers already converted the value with `str()`.

Consequence for 2.0: every reader must accept `true`, `True`, `1` (and `false`, `False`, `0`) as booleans, and JSON-list values may be JSON (`["x"]`) or Python repr (`['x']`). Existing readers in the old code: `login.py:66` accepts `"true"`, `True`, `"True"`; `rtos.py:231-234` lowercases; `modem_controller.py:844` lowercases; the UI's `castValue` lowercases (`FE/utilits/index.js:101-112`).

`updated_at` semantics (used by migrations and an OS script as "the user never changed this"):
- `appsettings.updated_at` / `devicesettings.updated_at` default to `utcnow()` at insert (`BE/models.py:167`, `:276`) and are bumped by `before_update` listeners (`BE/models.py:252-256`, `:336-340`). An upsert only writes when the value differs (`BE/crud.py:178`, `:384`).
- Rows that came from the seed DB (zigbee2mqtt defaults) have `updated_at = NULL`. Migration `ac49055b2b34` and `OS/usr/lib/smhub/set_default_zigbee_z2m.sh:39-52` only touch rows `WHERE updated_at IS NULL`. Keep that meaning: never write `updated_at` on rows you did not change.
- `apps.updated_at` is set by a `before_update` listener on every ORM update (`BE/models.py:247-249`); `pages.updated_at` likewise (`:329-333`).

### 1.3 Schema as it exists on devices

The seed DB (`PKG/CONTROL/db`, alembic head `4cc919e10054`) contains exactly this DDL (column order is historical; `ALTER TABLE` columns are appended):

```sql
CREATE TABLE alembic_version (
	version_num VARCHAR(32) NOT NULL,
	CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num)
);
CREATE TABLE user (
	email VARCHAR(255) NOT NULL,
	is_active BOOLEAN NOT NULL,
	is_superuser BOOLEAN NOT NULL,
	full_name VARCHAR(255),
	id VARCHAR NOT NULL,
	hashed_password VARCHAR NOT NULL,
	PRIMARY KEY (id)
);
CREATE UNIQUE INDEX ix_user_email ON user (email);
CREATE TABLE usersettings (
	id VARCHAR NOT NULL,
	light_theme BOOLEAN,
	metric_units BOOLEAN,
	notifications BOOLEAN,
	language VARCHAR(8),
	locale VARCHAR(8),
	timezone VARCHAR(64),
	use_12h_clock BOOLEAN,
	PRIMARY KEY (id),
	FOREIGN KEY(id) REFERENCES user (id)
);
CREATE TABLE apps (
	id INTEGER NOT NULL,
	"key" VARCHAR(32) NOT NULL,
	name VARCHAR(32) NOT NULL,
	description VARCHAR(1024),
	enabled BOOLEAN NOT NULL,
	start_at_boot BOOLEAN NOT NULL,
	created_at DATETIME NOT NULL,
	updated_at DATETIME, version VARCHAR(16) NOT NULL, watchdog BOOLEAN, installed BOOLEAN DEFAULT 0 NOT NULL, iframe_port INTEGER, icon VARCHAR(64), is_service BOOLEAN DEFAULT '0' NOT NULL, show_in_sidebar BOOLEAN DEFAULT '1' NOT NULL,
	PRIMARY KEY (id),
	UNIQUE ("key")
);
CREATE TABLE appsettings (
	id INTEGER NOT NULL,
	"key" VARCHAR(255) NOT NULL,
	value VARCHAR(1024) NOT NULL,
	updated_at DATETIME,
	app_id INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(app_id) REFERENCES apps (id),
	CONSTRAINT uix_app_id_key UNIQUE (app_id, "key")
);
CREATE INDEX ix_appsettings_updated_at ON appsettings (updated_at);
CREATE TABLE pages (
	id INTEGER NOT NULL,
	"key" VARCHAR(32) NOT NULL,
	name VARCHAR(32) NOT NULL,
	description VARCHAR(1024),
	created_at DATETIME NOT NULL,
	updated_at DATETIME,
	PRIMARY KEY (id),
	UNIQUE ("key")
);
CREATE TABLE devicesettings (
	id INTEGER NOT NULL,
	"key" VARCHAR(255) NOT NULL,
	value VARCHAR(1024) NOT NULL,
	updated_at DATETIME,
	page_id INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(page_id) REFERENCES pages (id),
	CONSTRAINT uix_dev_id_key UNIQUE (page_id, "key")
);
CREATE INDEX ix_devicesettings_updated_at ON devicesettings (updated_at);
CREATE TABLE fileitem (
	name VARCHAR(255) NOT NULL,
	is_directory BOOLEAN NOT NULL,
	path VARCHAR(1024) NOT NULL,
	parent_id VARCHAR,
	size INTEGER,
	mime_type VARCHAR(128),
	id VARCHAR NOT NULL,
	created_at DATETIME NOT NULL,
	updated_at DATETIME, synced_at DATETIME, owner VARCHAR(128) DEFAULT 'smlight' NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(parent_id) REFERENCES fileitem (id)
);
CREATE TABLE configsource (
	id INTEGER NOT NULL,
	name VARCHAR(64) NOT NULL,
	provider VARCHAR(32) NOT NULL,
	path VARCHAR(255) NOT NULL,
	params JSON,
	app_id INTEGER,
	PRIMARY KEY (id),
	FOREIGN KEY(app_id) REFERENCES apps (id)
);
CREATE UNIQUE INDEX ix_configsource_name ON configsource (name);
CREATE TABLE configsetting (
	id INTEGER NOT NULL,
	name VARCHAR(64) NOT NULL,
	source_name VARCHAR(64) NOT NULL,
	path VARCHAR(255) NOT NULL,
	value VARCHAR(1024),
	app_id INTEGER,
	PRIMARY KEY (id),
	FOREIGN KEY(app_id) REFERENCES apps (id),
	FOREIGN KEY(source_name) REFERENCES configsource (name) ON DELETE CASCADE
);
```

A device that started from an older DB and was migrated has the same columns but possibly in a different order (fresh `6e89cce31fda` puts `apps.version` before `description`, `BE/alembic/versions/00_consolidated_initial_schema.py:59-71`). Always address columns by name.

Model-level defaults (applied by the ORM at insert, not by SQLite) - `BE/models.py`:

| Table.column | Default |
|---|---|
| `user.is_active` / `is_superuser` / `full_name` | `True` / `False` / `NULL` (`:12-16`) |
| `usersettings` | `light_theme=1, metric_units=1, notifications=1, language='en', locale='en_US', timezone='UTC', use_12h_clock=0` (`:63-71`) |
| `apps` | `enabled=0, is_service=0, start_at_boot=0, watchdog=NULL, installed=0, show_in_sidebar=1, iframe_port=NULL, icon=NULL, created_at=utcnow, updated_at=NULL` (`:133-149`); `AppsCreate.watchdog` defaults to `False` so newly created rows get `watchdog=0` (`:180`) |
| `appsettings` / `devicesettings` | `updated_at=utcnow` (`:167`, `:276`) |
| `pages` | `created_at=utcnow, updated_at=NULL` (`:259-265`) |
| `fileitem` | `owner='smlight', created_at=utcnow, updated_at=utcnow, synced_at=NULL` (`:343-357`) |

Relationships with ORM cascade delete (performed by SQLAlchemy, not by SQLite): `user -> usersettings`, `apps -> appsettings/configsource/configsetting`, `pages -> devicesettings` (`BE/models.py:50`, `:151-159`, `:266-268`). Deleting a page via `DELETE /pages/{key}` therefore deletes its `devicesettings` rows (`BE/api/routes/pages.py:110-129`).

### 1.4 Seed data (`PKG/CONTROL/db`)

| Table | Rows |
|---|---|
| `alembic_version` | `4cc919e10054` |
| `user` | one row: `email='admin@smlight.tech'`, `is_active=1`, `is_superuser=1`, `full_name=NULL`, `id='9020dd38-d19e-4ef4-b7dc-61135f38930e'`, `hashed_password` = a **bcrypt** `$2b$12$...` hash of the factory default password (value not reproduced). The same id and hash are on every device. |
| `usersettings` | `('9020dd38-...', 1, 1, 1, 'en', 'en-US', 'Australia/Melbourne', 1)` |
| `apps` | 8 rows, all `installed=0, enabled=0` except zigbee2mqtt `enabled=1`, `start_at_boot=0`, `watchdog=NULL`, `iframe_port=NULL`, `icon=NULL`, `is_service=0`, `show_in_sidebar=1`: `matterbridge-z2m 2.4.7-2`, `matterbridge 3.0.2-1`, `nodejs 22.13.1-1`, `nodered 4.0.9-1`, `zigbee2mqtt 2.3.0-1`, `zwavejsui 10.4.0-1`, `smhub-services 0.1.12-1`, `openthread 0.3.0-1` (ids 1..8 in that order) |
| `appsettings` (app 5 = zigbee2mqtt, `updated_at=NULL`) | `serial_port=/dev/ttyS1`, `baud_rate=115200`, `adapter=zstack`, `zigbee_channel=11`, `mqtt_broker_url=mqtt://localhost:1883`, `base_topic=zigbee2mqtt` |
| `pages` | `(1, 'esphome', 'ESPHome', 'Smart Home Hub RTOS settings', '2026-06-13 11:25:47.950819', NULL)` |
| `devicesettings` (page 1) | `ambient_light=true`, `buzzer=true`, `user_i2c=true`, `bluetooth_proxy_mode=hci`, `encryption_key=<factory-default ESPHome key, base64; the same constant as BE/rtos.py:16>` |
| `configsource`, `configsetting`, `fileitem` | empty (filled at first start) |

`FIRST_SUPERUSER` / `FIRST_SUPERUSER_PASSWORD` from `BE/.env` are only used by `init_db` (`BE/core/db.py:56-76`, called from `BE/initial_data.py`), which is not run on devices. The device's admin comes from the seed DB.

### 1.5 What the code reads and writes, per table

| Table | Written by | Read by |
|---|---|---|
| `user` | `ensure_system_user` at every start (`BE/core/db.py:41-53`); signup, `PATCH /users/me`, `PATCH /users/me/password`, `PATCH/DELETE /users/{id}`, `DELETE /users/me` (section 20); password rehash on login (`BE/crud.py:84-87`) | auth (`BE/api/deps.py:28-76`), `/login/*` |
| `usersettings` | created with each user (`BE/crud.py:37-39`); `POST /smhub/user/settings` (`BE/api/routes/data.py:46-67`, partial update, `exclude_unset`) | `GET /smhub/user/settings` (`BE/api/routes/data.py:24-43`) |
| `apps` | startup `check_and_create_apps`, `sync_app_autostart`, `sync_app_metadata`; install/remove routes; WS `app_control` | `/smhub/packages/state`, iframe proxy, services status |
| `appsettings` | startup `sync_app_settings` (missing keys only); `POST /smhub/packages/{pkg}/settings`; reset (delete all + `adapter`); startup serial-port correction | `GET /smhub/packages/{pkg}/settings`, serial-port correction |
| `pages` | only migration `4cc919e10054` creates a page (`esphome`); all other pages are created by the UI with `POST /pages` before saving (`FE/components/Settings/SchemaForm.jsx:219-242`, `FE/pages/Settings/AuthorizationSettings.jsx:26-40`, `FE/pages/Settings/Radio.jsx:54-69`, `FE/pages/Settings/Ser2net.jsx:66-75`) | page routes, RTOS startup sync, modem start (`lte`), `/login/security-status` (`security`) |
| `devicesettings` | page routes; startup `sync_pages_autostart` | same |
| `configsource` | startup `sync_app_metadata` (per app `system.config_sources`) and `sync_system_schema` (built-ins, `app_id=NULL`); install route | startup copies all rows into the in-memory registry (`BE/main.py:237-266`) |
| `configsetting` | startup `sync_app_settings` and `sync_system_schema`; serial-port correction | startup registry copy |
| `fileitem` | file manager routes and sync (section 9) | file manager |

Saving page settings requires the `pages` row: `crud.update_page_settings` raises `ValueError` -> HTTP 404 if the page key does not exist (`BE/crud.py:408-410`, `BE/api/routes/pages.py:188-190`). 2.0's UI does not create pages, so 2.0 creates a missing page row itself when it saves, using the keys in section 4.

### 1.6 How `configsource` / `configsetting` are filled from `schema.json`

Package schema files are `/var/lib/opkg/info/<pkg>.schema.json` (`SC/packages.py:603-621`; copied there by opkg from each app's `CONTROL/schema.json`). They are loaded as plain JSON into an attribute-accessible dict (`SC/packages.py:70-106`, `:593-601`).

At startup, for every package that is installed (`version is not None`) (`BE/main.py:205-222`):

1. `sync_app_settings(pkg, schema)` (`BE/core/db.py:164-232`), for each `k, v` in `schema.config`:
   - If no `appsettings` row `(app_id, k)` exists: insert `value = v.default` if `v` is an object, else `""`. Separator entries (no `default`) get a row with `value=""`. Existing rows are never overwritten.
   - If `v` has `path`: upsert `configsetting(name=k, source_name=v.source or <pkg>, path=v.path, value=v.value or v.default, app_id)`; key for the upsert is `(name, source_name)` (`BE/crud.py:253-286`).
   - If `v.toggle` is an object with `path` and `field`: upsert `configsetting(name=toggle.field, source_name=v.source or <pkg>, path=toggle.path, value=v.value or v.default or toggle.default)`. The toggle field does **not** get an `appsettings` row.
2. `sync_app_metadata(pkg, schema)` (`BE/core/db.py:106-161`):
   - `apps.iframe_port = system.iframe_port.default` if it is an int (0 is stored as 0).
   - `apps.is_service = system.service.default` if it is a bool.
   - For each object in `system.config_sources` with a `name`: upsert `configsource(name, provider=provider or "json", path=path or "", params=params, app_id)` keyed by `name` (`BE/crud.py:224-250`). Upsert overwrites provider/path/params/app_id.
   - `apps.icon` is **not** set (the docstring says so, the code does not).

Then `sync_system_schema()` (`BE/core/db.py:235-272`) loads `BE/core/system_schema.json` and upserts, with `app_id=NULL`:

| configsource name | provider | path | params |
|---|---|---|---|
| `mosquitto` | `flat` | `/etc/mosquitto/mosquitto.conf` | `{"comment_prefixes": ["#"]}` |
| `hostname` | `single` | `/etc/hostname` | null |
| `timezone` | `single` | `/etc/timezone` | null |
| `usb_mode` | `single` | `/etc/usb_mode` | null |

| configsetting name | source | path | value (=default) |
|---|---|---|---|
| `hostname` | hostname | `hostname` | `smhub` |
| `timezone` | timezone | `timezone` | `UTC` |
| `usb_mode` | usb_mode | `usb_mode` | `device` |
| `listener_port` | mosquitto | `listener` | `1883` |
| `bind_interface` | mosquitto | `bind_interface` | `localhost` |
| `persistence_file` | mosquitto | `persistence_file` | `mosquitto.db` |
| `persistence_location` | mosquitto | `persistence_location` | `/var/lib/mosquitto/` |
| `log_dest` | mosquitto | `log_dest` | `file /var/log/mosquitto.log` |
| `password_file` | mosquitto | `password_file` | `/var/lib/mosquitto/passwd` |
| `allow_anonymous` | mosquitto | `allow_anonymous` | `false` |
| `remote_connection` | mosquitto | `connection` | `` |
| `remote_address` | mosquitto | `address` | `` |
| `remote_username` | mosquitto | `remote_username` | `` |
| `remote_password` | mosquitto | `remote_password` | `` |
| `clean_session` | mosquitto | `cleansession` | `true` |
| `bridge_topic` | mosquitto | `topic` | `` |

(`BE/core/system_schema.json:1-45`.)

`configsetting.value` is reset to the schema default on every start and is never updated when the user saves settings; nothing reads it for file I/O (the provider classes ignore `ConfigSetting.value`). It is informational only.

The in-memory registry (`SC/data.py:138-152`) is cleared and refilled from these two tables once per process, after the steps above (`BE/main.py:237-266`). `ProviderController(name)` looks a source up by name in that registry and instantiates the provider class for `provider` (`SC/provider.py:26-35`, `:514-519`). It is **not** refreshed after an install at runtime (bug 3 in "Bugs and oddities").

Real `config_sources` and config keys shipped by apps (`APPS/*/CONTROL/schema.json`):

| App | config_sources | config keys with `path` (default) |
|---|---|---|
| zigbee2mqtt | `zigbee2mqtt`, yaml, `/opt/zigbee2mqtt/data/configuration.yaml` | `serial_port` -> `serial.port` (`/dev/ttyS1`), `baud_rate` -> `serial.baudrate` (115200), `adapter` -> `serial.adapter` (zstack), `flow_control` -> `serial.rtscts` (false), `zigbee_channel` -> `advanced.channel` (15), `mqtt_broker_url` -> `mqtt.server`, `mqtt_user` -> `mqtt.user`, `mqtt_password` -> `mqtt.password`, `base_topic` -> `mqtt.base_topic`, `client_id` -> `mqtt.client_id`, `permit_join` -> `permit_join`, `log_level` -> `advanced.log_level`, `transmit_power` -> `advanced.transmit_power` (20), toggle `ha_enabled` -> `homeassistant.enabled`, `ha_experimental_events` -> `homeassistant.experimental_event_entities`, `ha_discovery_topic` -> `homeassistant.discovery_topic`, `ha_status_topic` -> `homeassistant.status_topic` |
| openthread | `openthread`, flat, `/opt/openthread/otbr-agent.conf` | `serial_port` -> `OTBR_SERIAL_PORT` (`/dev/ttyS2`), `baud_rate` -> `OTBR_BAUDRATE` (460800), `log_level` -> `LOG_LEVEL` (notice), `firewall` -> `FIREWALL` (true), `nat64` -> `NAT64` (true) |
| zwavejsui | `zwavejsui`, json, `/opt/zwavejsui/.zwave-js-ui/settings.json` | `enabled` -> `zwave.enabled`, `port` -> `zwave.port` (`/dev/ttyS3`), `region` -> `zwave.rf.region`, `S2_Unauthenticated_key`, `S2_Authenticated_key`, `S2_AccessControl_key`, `S0_Legacy_key` -> `zwave.securityKeys.*`, `S2_Authenticated_lr_key`, `S2_AccessControl_lr_key` -> `zwave.securityKeysLongRange.*` |
| picoclaw-launcher | `picoclaw`, json, `/home/smlight/.picoclaw/launcher-config.json` | `launcher_token` -> `launcher_token` (no `source`, so it is linked to source `picoclaw-launcher`, which does not exist) |

---

## 2. Authentication

### 2.1 Password hashing

Implemented in Rust (`RS/security.rs`, PyO3 module `smhub_rs`) and called from `BE/core/security.py:10-15`.

- New hashes: Argon2 with the `argon2` crate 0.5 `Argon2::default()` = **Argon2id, v=19, m=19456 KiB, t=2, p=1**, 32-byte output, random 16-byte salt from `SaltString::generate(OsRng)`, PHC string `$argon2id$v=19$m=19456,t=2,p=1$<salt b64 no pad>$<hash b64 no pad>` (`RS/security.rs:33-42`; parameters confirmed by the test vector at `:183`).
- Verify: if the stored hash starts with `$2b$`, `$2a$` or `$2y$`, verify with bcrypt (`bcrypt` crate 0.17); otherwise parse as a PHC string and verify with `Argon2::default()` (parameters come from the string). Unparseable hash = false (`RS/security.rs:44-56`).
- Rehash on login: after a successful login, if the stored hash is bcrypt, it is replaced by a new Argon2id hash and committed (`BE/crud.py:78-88`). Only `crud.authenticate` (the two login routes) rehashes; `PATCH /users/me/password` verifies with the same function but always stores Argon2id for the new password.
- Password length rules on input models: 8..40 characters for create, register, update and change (`BE/models.py:20-43`, `:124-126`).

### 2.2 JWT

- Algorithm HS256, header `{"typ":"JWT","alg":"HS256"}` (`jsonwebtoken` 10 `Header::default()`), claims exactly `{"sub": "<user.id>", "exp": <unix seconds>}` (`RS/security.rs:27-31`, `:58-78`).
- Lifetime: `ACCESS_TOKEN_EXPIRE_MINUTES = 60*24*8` = 8 days = 691200 s (`BE/core/config.py:32`, `BE/api/routes/login.py:31`).
- Verify: `Validation::new(HS256)` - checks signature and `exp` (default leeway 60 s, **Inferred** from jsonwebtoken defaults), no `iss`/`aud`; returns `sub` (`RS/security.rs:80-92`).
- `SECRET_KEY` source: pydantic-settings reads env var `SECRET_KEY`, else the `.env` file at `BE/.env` (resolved as `Path(core/config.py).parent.parent / ".env"`, i.e. on the device `/opt/smhub-services/venv/lib/python3.X/site-packages/smhub_backend/.env`) (`BE/core/config.py:23-30`). The shipped `.env` defines `SECRET_KEY` (an opaque string, value not reproduced), so **every device uses the same key**. (The file is tracked in git and inside the wheel's `only-include = ["/smhub_backend"]`; it must be present on devices because `PROJECT_NAME`, `FIRST_SUPERUSER` and `FIRST_SUPERUSER_PASSWORD` have no defaults and `Settings()` would fail without it, `BE/core/config.py:47`, `:63-64`, `:88`.) If neither exists, a random key is generated per process start (`secrets.token_urlsafe(32)`, `:30`) and all tokens die at restart.
  - Compatibility: a cookie issued by 1.x is valid only for a backend with the same key. 2.0 does not read these tokens; it has its own sessions (D41), so users with auth enabled sign in once more after the update (a hub with auth disabled needs no sign-in on 2.0).

### 2.3 Login and token transport

| Endpoint | Input | Success | Failure |
|---|---|---|---|
| `POST /api/v1/login/cookie` | form `application/x-www-form-urlencoded` (`username`=email, `password`; OAuth2PasswordRequestForm) | `200`, body `Login successful` (text/plain), `Set-Cookie: access_token=<jwt>; Max-Age=691200; Path=/; SameSite=strict` - **not HttpOnly, not Secure** (`BE/api/routes/login.py:16-43`) | `401 {"detail":"Incorrect email or password"}`, `400 Inactive user` |
| `POST /api/v1/login/access-token` | same form | `{"access_token": "<jwt>", "token_type": "bearer"}` (`:70-89`) | `400` for wrong credentials |
| `GET` or `HEAD` `/api/v1/login/user-session` | token | `UserPublic` `{email,is_active,is_superuser,full_name,id}` (`:46-55`) | 401/403/404/400 as below. **Used by the OS `api-ready` script; must stay.** |
| `POST /api/v1/login/test-token` | token | `UserPublic` (`:92-97`) | |
| `GET /api/v1/login/security-status` | none (no auth) | `{"enabled": <bool>}` = `devicesettings` of page `security`, key `enabled`, value in (`"true"`, `"True"`) (`:58-67`) | |

Resolving the current user (`BE/api/deps.py:47-76`):
1. If the request came over the Unix socket -> user `system@localhost` (2.5).
2. Token = cookie `access_token`; else header `Authorization: Bearer <token>`.
3. No token -> `401 {"detail":"Not authenticated"}` with `WWW-Authenticate: Bearer`.
4. Decode failure -> `403 {"detail":"Could not validate credentials"}`.
5. `user.id == sub` not found -> `404 User not found`; `is_active` false -> `400 Inactive user`.

`check_current_user` (`BE/api/deps.py:90-96`) is just "authenticated". `get_current_active_superuser` adds `403 The user doesn't have enough privileges` (`:82-87`).

### 2.4 The `security` page and "auth disabled"

- The backend has **no** auth switch. Every protected endpoint always needs a token or the Unix socket.
- `devicesettings(page='security', key='enabled')` only drives the UI: if not enabled, the UI silently calls `/login/cookie` with the factory admin e-mail `admin@smlight.tech` and the factory default password hard-coded in `FE/App.jsx:81-85` (value not reproduced) and continues (`FE/App.jsx:60-104`).
- Enabling auth in the UI: saves `security` page `{enabled: "true"|"false", email}` and, if e-mail and password were typed, `POST /users/signup` creates a new **non-superuser** account (`FE/pages/Settings/AuthorizationSettings.jsx:67-123`). The factory admin keeps its default password, so the "auth enabled" state is only as strong as that.
- Saving the `security` page also runs `SecurityPageHandler` (rewrites `sshd_config` and restarts `sshd`, 4.10).

### 2.5 `system@localhost` and the Unix socket

- At every start `ensure_system_user` creates `user(email='system@localhost', is_superuser=0, password=<random token_urlsafe(24)>, hashed with Argon2id)` plus a `usersettings` row, if missing (`BE/core/db.py:41-53`).
- The Unix socket `/run/smhub-backend.sock` is served by a second uvicorn server on the same app with `lifespan="off"`; an existing socket file is unlinked first; created with default permissions (**Inferred**: root-owned, mode from umask) (`BE/main.py:312-334`).
- Rule: `is_uds_request` is true when `scope["client"]` is `None` or `scope["server"]` is missing, or `scope["server"][1]` (port) is `None` (`BE/api/deps.py:39-44`). Such requests are authenticated as `system@localhost` (401 if that user is missing or inactive) (`:28-36`) without any token. It is not a superuser.

### 2.6 Mosquitto password file

- Endpoint `POST /api/v1/users/create-mqtt-user`, JSON body `{"username": "...", "password": "..."}`, **no authentication** (`BE/api/routes/users.py:208-222`). Calls `smhub_rs.create_mqtt_user` (`RS/security.rs:166-170`).
- Entry format: `<username>:$7$1000$<base64(salt)>$<base64(hash)>` where salt = 12 random bytes, hash = PBKDF2-HMAC-SHA512, 1000 iterations, 64-byte output, standard Base64 **with** padding (`RS/security.rs:94-109`). This is mosquitto's own `$7$` format, so `mosquitto_passwd` and mosquitto can read it.
- Username may not contain `:`, `\n`, `\r` (`:95-97`); password not validated.
- File `/var/lib/mosquitto/passwd`: existing lines are trimmed, empty lines dropped, any line whose part before the first `:` equals the username dropped; the new entry appended; joined with `\n` plus a final `\n` (`:111-133`). Parent dir created if missing. Written to `/var/lib/mosquitto/passwd.tmp.<pid>` and renamed over the file (`:147-163`). Result: file owned by root with umask permissions (the original `mosquitto:mosquitto 0600` is lost; **Inferred**).
- Mosquitto is not signalled or restarted, so the new user only works after mosquitto re-reads the file (restart or SIGHUP; **Inferred**). The `mosquitto_passwd` CLI command defined in `SC/data.py:88-93` is never used.

---

## 3. Apps / packages

### 3.1 opkg data sources

| Data | Source | Code |
|---|---|---|
| Available packages | every file in `/var/lib/opkg/lists/*` (gzip if suffix `.gz`) | `SC/packages.py:153-237` |
| Installed packages + version | `/var/lib/opkg/status`; fallback `opkg list-installed` parsed with `^(\S+) - (\S+)$` | `SC/packages.py:331-415` |
| Schema | `/var/lib/opkg/info/<pkg>.schema.json` | `SC/packages.py:603-621` |
| Changelog | `/var/lib/opkg/info/<pkg>.CHANGELOG.md` | `SC/packages.py:582-591` |
| postinst for reset | `/var/lib/opkg/info/<pkg>.postinst` | `SC/packages.py:517-528` |
| Fast "is installed" | `/var/lib/opkg/info/<pkg>.control` or `.list` exists, else `check_autostart` | `SC/packages.py:735-743` |

Index parsing (`SC/packages.py:187-237`): split content on 2+ newlines; each line with `:` is `key: value`, key lowercased and trimmed, value trimmed (continuation lines are not handled). An entry needs `package` and `version`. Package `smhub-web` is skipped. If the same package is in several feeds, the one with the higher version (3.2) wins. Fields kept: `package, version (as latest_version), description, priority, depends, architecture, maintainer, md5sum, size (int), filename, required-os-version, section, homepage`, plus `version` = installed version or null (`SC/packages.py:48-67`). The parsed list is cached until the newest mtime of the list files changes (`:165-178`).

Status parsing (`SC/packages.py:331-389`): blocks separated by blank lines; on `Package:` start a block; while still in the header (`Status:`, `Version:` and the allowed fields `Essential, Section, Architecture, Installed-Time, Maintainer, Auto-Installed, Depends, Provides, Conffiles`) take `Status:` containing the substring `installed` and `Version:`. Note `not-installed` also contains `installed` (bug list).

### 3.2 Version comparison (`SC/packages.py:252-329`)

1. Replace every `-` by `.`.
2. Tokenise: `.` separates tokens; `~` is its own token of kind `tilde`; everything else forms `normal` tokens.
3. Compare token by token; a missing token counts as `normal "0"`.
4. `tilde` < `normal`. Two normal tokens: if both parse as integers compare numerically, else compare as strings (byte order).

Examples: `1.0.0~rc1 < 1.0.0`, `1.1.18-1~dev1 < 1.1.18-1`, `1.0.10 > 1.0.9`, `1.0 == 1.0.0`.

### 3.3 Commands and their logging

All four opkg actions run as `opkg <args>` with the environment of the backend, `stdout` appended to `/var/log/opkg.log` and `stderr` merged into it, in a **new session** (`start_new_session=True`) so they survive the backend being killed (e.g. when `smhub-services` itself is upgraded) (`SC/packages.py:429-465`). Non-zero exit -> error. Afterwards the package list is reloaded.

| Action | Command | Route (all GET, auth required) | DB / file effects after success |
|---|---|---|---|
| install | `opkg install <pkg>` (`SC/packages.py:467-481`) | `/smhub/packages/{pkg}/install` (`BE/api/routes/packages.py:179-219`) | `check_and_create_apps`; `apps.installed=1`; `sync_app_settings` + `sync_app_metadata` (with hot-patched schema); iframe proxy `process_app` (3.11) |
| remove | `opkg remove <pkg>` (`:483-496`) | `/smhub/packages/{pkg}/remove` (`:222-251`) | `apps.installed=0`; `process_app` (deletes stunnel file). `apps` row and its `appsettings` are kept. |
| update | `opkg update` (`:573-580`) | `/smhub/packages/update` (`:55-65`) | none (404 on failure) |
| upgrade | `opkg upgrade` (all packages) (`:564-571`) | `/smhub/packages/upgrade` (`:68-78`) | none (404 on failure) |

The package must exist in the index (else 404) for install/remove/reset/run/isRunning/log-history.

### 3.4 `apps` rows

`check_and_create_apps(pkg)` (`BE/core/db.py:79-103`), called at startup for **every package in the feed index** (not only apps):
- Missing row: insert `key=name=<pkg>`, `description=<index description>`, `version=<latest_version from index>`, `installed = (installed version is not null)`.
- Existing row: only `installed` is synced. `version`, `name`, `description` are never updated afterwards.

Other writes to `apps`: `start_at_boot` (startup `sync_app_autostart` and WS `app_control`), `watchdog` and `show_in_sidebar` (WS `app_control`), `iframe_port`, `is_service` (`sync_app_metadata`). `enabled` and `icon` are never written by code.

Responses (for reference): `GET /smhub/packages/state` and `/{pkg}/state` return the `apps` row plus `iframe_https_port` (from the stunnel port map) and `start_at_boot` recomputed live from `check_autostart` (`BE/api/routes/packages.py:81-97`, `:134-153`).

### 3.5 `schema.json` keys understood by the backend

| Key | Used by | Effect |
|---|---|---|
| `system.data_dir.default` (string or list of strings) | backup (8), file manager mounts (9), reset (3.7, string only) | |
| `system.iframe_port.default` (int) | `apps.iframe_port` (3.4); stunnel (3.11); icon route requires it > 0 | |
| `system.service.default` (bool) | `apps.is_service`; services status list (14) | |
| `system.service.name` (string, only if `system.service` is an object with `name`) | OpenRC service name for start/stop/autostart/isRunning (3.8) | `SC/packages.py:623-641` |
| `system.icons.<type>` (string, or object with `default`) | `GET /smhub/packages/{pkg}/icon?type=<type>` (default `icon`); file resolved relative to `/opt/<pkg>/`, must stay inside it and exist; served as `image/svg+xml` when `.svg`, with `X-Content-Type-Options: nosniff`; only if `iframe_port > 0` (`SC/packages.py:651-695`, `BE/api/routes/packages.py:455-476`) | |
| `system.config_sources[]` `{name, provider, path, params}` | `configsource` rows (1.6) | |
| `system.watchdog`, `system.config_reset` | not read by the backend (UI only) | |
| `config.<key>.default` | initial `appsettings` value (1.6) | |
| `config.<key>.path` / `.source` / `.value` | `configsetting` rows (1.6) | |
| `config.<key>.toggle.{field,path,default}` | `configsetting` row for the toggle field (1.6) | |
| `config.<key>.type`, `options`, `enum`, `pattern`, `pattern_message`, `required`, `min`, `max`, `placeholder`, `hiddenBy`, `read-only`, `label`, `data_description` | UI only; `serial_port` is hot-patched (3.6) | |
| `actions`, `status` (e.g. tailscale, openthread) | UI only (they name endpoints such as `/smhub/tailscale/authenticate`) | |

`GET /smhub/packages/{pkg}/schema` returns the hot-patched schema or `{}` (`BE/api/routes/packages.py:156-176`).

### 3.6 Hot-patching `serial_port` (zigbee2mqtt, openthread)

`get_hotpatched_schema(pkg, eeprom)` (`BE/core/db.py:413-485`), only for `openthread` and `zigbee2mqtt` with `config.serial_port`:
- `type = "enum"`.
- Model `nano*` but not `nanomr*`: `default = "/dev/ttyS1"`, `pattern = "^/dev/ttyS1$|^/dev/ttyUSB.*$|^/dev/serial/by-id/.*$"`, `pattern_message = "serial_port_nano_only"`, internal ports `[/dev/ttyS1]`.
- Model `nanomr*`: `pattern = "^/dev/ttyS(1|2)$|^/dev/ttyUSB.*$|^/dev/serial/by-id/.*$"`, `pattern_message = "serial_port_mr_only"`, internal ports `[/dev/ttyS1, /dev/ttyS2]` (default unchanged).
- Other models: internal ports `[/dev/ttyS1, /dev/ttyS2]`.
- USB ports: output lines of `ls -1 /dev/serial/by-id` (command `list_serial_by_id`, `SC/data.py:82-87`) - these are **bare names**, not full paths; empty if the directory is missing.
- `options = enum = unique(internal + usb)`.

Startup correction `sync_model_serial_port_corrections` (`BE/core/db.py:319-410`), only for models starting with `nano`:
- Read `appsettings.serial_port` of zigbee2mqtt and openthread. Needs correction if: it starts with `/dev/ttyS` and is not allowed (`nano*`: only `/dev/ttyS1`; `nanomr*`: `/dev/ttyS1` or `/dev/ttyS2`), or it is neither `/dev/ttyS*`, `/dev/ttyUSB*` nor `/dev/serial/by-id/*`.
- Correction: set `appsettings.serial_port = '/dev/ttyS1'`; set every `configsetting(app_id, name='serial_port').value = '/dev/ttyS1'`; then `ProviderController(<app>).bulk_set({"serial_port": "/dev/ttyS1"}); save()`. The last step **fails silently today** because the registry is still empty at that point of startup (bug 1), so only the DB is corrected.

### 3.7 Reset config

`GET /smhub/packages/{pkg}/reset` (`BE/api/routes/packages.py:292-325`):
1. Delete all `appsettings` rows of the app (`BE/crud.py:209-221`).
2. Only for `zigbee2mqtt`: upsert `appsettings.adapter = "ember"` if the lowercased EEPROM model contains `nanomg` or equals `nanomr5`, else `"zstack"` (also `zstack` without EEPROM) (`BE/api/routes/packages.py:254-289`).
3. `Packages.reset_config` (`SC/packages.py:498-528`): load schema; if `system.data_dir.default` is a string and an existing directory: `shutil.rmtree(<data_dir>)` (lists are skipped). If the schema has no `system.data_dir` the function **returns here** without running postinst. Then, if `/var/lib/opkg/info/<pkg>.postinst` exists, run it with no arguments, capture output; non-zero exit -> 500.
4. Services are not stopped or restarted. The other `appsettings` rows come back only at the next backend start (`sync_app_settings`).

### 3.8 Service name, start/stop, isRunning, autostart, watchdog

- Service name: `schema.system.service.name`, else `SERVICE_MAPPING` (`smhub-ui` -> `nginx`), else the package name (`SC/packages.py:623-641`, `SC/data.py:22-24`).
- OpenRC detection: `/sbin/openrc-run` exists (`SC/initManager.py:56-59`). Commands: `rc-service <svc> <action>` with stdin `/dev/null`, stdout/stderr captured (`SC/initManager.py:61-162`). Non-zero exit raises.
- Start/stop: `GET /smhub/packages/{pkg}/run?stop=false|true` -> `rc-service <svc> start|stop`; legacy non-OpenRC: `/etc/init.d/S99<svc> start|stop` (`openthread` -> `otbr-agent`) (`SC/packages.py:530-562`).
- isRunning: `rc-status -a -f ini` parsed with configparser (`allow_no_value`), for each `key = value` the first word of the value; `started` = running (`SC/initManager.py:164-194`, `SC/packages.py:643-649`). `GET /smhub/packages/{pkg}/isRunning` -> `{"isRunning": bool}`.
- Autostart set (`SC/packages.py:697-723`): flag file `/opt/share/services/<pkg>.autostart` (empty file; created if missing when enabling, deleted when disabling) **and** `rc-update add|del <svc> default` (exit code ignored).
- Autostart check (`SC/packages.py:725-733`): if `/etc/init.d/<svc>` exists -> `/etc/runlevels/default/<svc>` exists; else the flag file exists.
- Startup `sync_app_autostart` writes `apps.start_at_boot = check_autostart(pkg)` for installed apps when different (`BE/core/db.py:275-292`).
- Watchdog: flag file `/opt/share/services/<pkg>.watchdog` only (`SC/packages.py:745-748`); read by app init scripts (zigbee2mqtt).
- Both are set over the WebSocket `app_control` message (`BE/api/routes/websocket.py:256-283`): `{"event":"app_control","data":{"appId":"<pkg>","settings":{"start_at_boot":bool,"watchdog":bool,"show_in_sidebar":bool}}}`; each present bool is applied (flag/rc-update) and written to `apps`.
- Both flag files are used by OS scripts (section 0 table), so keep writing them.

### 3.9 Generic app settings

- `GET /smhub/packages/{pkg}/settings` -> `{key: value}` of `appsettings` (`BE/api/routes/packages.py:368-381`).
- `POST /smhub/packages/{pkg}/settings` with a JSON object (`:384-413`):
  1. DB: for each key, `str(value)` upsert (1.2 table) - creates rows for unknown keys too; 404-style `ValueError` if the app row is missing becomes a 500.
  2. Config file: `ProviderController(<pkg>)` (needs a `configsource` named exactly like the package) -> `bulk_set(raw JSON body)` -> `save()` (section 4.1). Any error -> `500 "Error writing config <pkg>"`.
  3. No service restart.

### 3.10 Logs and changelog

- `GET /smhub/packages/{pkg}/changelog` -> `{"package", "changelog": <file text>}`; 404 if missing.
- Logs: section 11.

### 3.11 iframe HTTPS proxy (stunnel)

`BE/core/iframe_proxy.py` (singleton). Constants: offset 20000, ports 1024..65535, 64 probe steps; dir `/opt/smhub-services/config/stunnel.conf.d`; static config `/etc/stunnel/stunnel.conf`.

Per-app file `/opt/smhub-services/config/stunnel.conf.d/app-<key>.conf`, exact content (`:79-86`):

```
; Auto-generated by smhub-backend. Do not edit manually.
[app-<key>]
accept = 0.0.0.0:<https_port>
connect = 127.0.0.1:<iframe_port>
```
(four lines, each ending in `\n`; written with `write_text`, root-owned, umask mode).

Port choice `_next_https_port(base, used)` (`:34-50`): start at `base + 20000` (if > 65535, start at 65535 and go downwards); try up to 64 candidates; skip candidates `== base` or in `used`; stop if out of 1024..65535; fall back to `base`.

`init_on_startup` (`:99-154`), at every backend start:
1. `static_port_map` = sections `[app-<key>]` with a line `accept = 0.0.0.0:<port>` in `/etc/stunnel/stunnel.conf` (`:52-67`). The shipped file has only `[https]`, so it is normally empty.
2. Read existing `app-*.conf` files; keep a port for app `<key>` if the app is installed with `iframe_port > 0`, not static, the port is in range, differs from `iframe_port`, and is not used yet.
3. For all apps with `iframe_port > 0` (installed or not), sorted by key, not static: allocate a port if none; write the file only for installed apps (always rewrites).
4. Delete `app-*.conf` files of apps that are not installed-with-port.
5. `rc-service stunnel restart` as a background task - **every start**, changed or not.

`process_app(app)` after install/remove (`:161-205`): static apps ignored; allocate if needed; installed -> write file if content differs; not installed or no port -> remove file and forget the port; restart stunnel only if a file changed.

`get_port_map()` = static + dynamic; reported as `iframe_https_port`.

The init script creates the directory with mode 755 before start (`PKG/CONTROL/openrc:28`).

---

## 4. Settings pages and config files

### 4.1 Config file providers (how every config file is read and written)

`ProviderController(name)` picks the provider by `configsource.provider` (`SC/provider.py:26-35`, `:514-519`). A provider loads the whole file in its constructor, `bulk_set` changes values, `save` rewrites the whole file. `bulk_set(dict)` accepts setting **names** or setting **paths** as keys; unknown keys are logged and ignored (`SC/provider.py:107-121`). Only `configsetting` rows whose `source_name` equals the source are known to a provider (`:62-64`).

#### `yaml` (`SC/provider.py:145-194`, `RS/config.rs`)

- Load: `smhub_rs.load_yaml_file(path)` -> parse with `serde-saphyr` into `serde_json::Value`; missing file = `{}`.
- Set (`SC/provider.py:89-105`): path split on `.`; intermediate keys created as maps (a non-map intermediate value is replaced by `{}`); **value `""` deletes the key**; other values stored with their JSON type (bool stays bool, number stays number).
- Save: `smhub_rs.save_yaml_file(path, data, header=params.header)` (`RS/config.rs:18-47`, `:57-92`):
  - serialise with `serde_saphyr::to_string(serde_json::Value)`. `serde_json` is built **without** `preserve_order` (`fastapi-backend/backend/Cargo.lock`, no `indexmap` dependency), so mappings are `BTreeMap`: **all keys are written in alphabetical order** and comments/anchors from the original file are lost (**Inferred** from the lock file).
  - header handling: header trimmed at the end; if the body starts with `---\n` and the header ends with `---`, drop that `---` from the header; else if the header starts with `%YAML` and does not end with `---`, append `\n---`; output = `header + "\n" + body`, or just the body if there is no header.
  - write to `<path minus extension>.tmp.<pid>` (Rust `with_extension`, e.g. `configuration.tmp.1234`) and rename over the file; parent directories created. New file: root-owned, umask mode.
- Files written this way: zigbee2mqtt `/opt/zigbee2mqtt/data/configuration.yaml` (app settings), `/etc/ser2net/ser2net.yaml` (4.11).

#### `json` (`SC/provider.py:197-213`)

- Load: `json.load`; a missing file raises (so `ProviderController` fails).
- Set: as yaml (dotted path, `""` deletes the key).
- Save: `json.dump(data, f, indent=2)` - Python defaults: `ensure_ascii=True`, key order preserved from the file, `": "` key separator, newline+2-space indentation, **no trailing newline**; opened with `"w"` (truncate in place, ownership kept).
- Files: zwave-js-ui `/opt/zwavejsui/.zwave-js-ui/settings.json`, picoclaw `/home/smlight/.picoclaw/launcher-config.json`.

#### `flat` (`SC/provider.py:216-268`, `SC/flatconf.py`)

Line-preserving editor for `key value` / `key=value` / `key: value` files. `params.comment_prefixes` (list or string; default `("#", ";")`).

Load (`SC/flatconf.py:45-87`), per line (newline stripped):
- empty/whitespace -> `blank` (kept verbatim);
- starts (after strip) with a comment prefix and the text right after the prefix matches `^[A-Za-z0-9._-]+\s*` -> `disabled_entry` (a commented-out setting, e.g. `#listener`, `#password_file`, `#OTBR_SERIAL_PORT=x`);
- other comment lines -> `comment` (kept verbatim, e.g. `# Config file for mosquitto`);
- otherwise -> `entry`.
- Split (`:89-109`): regex `^([^=:\s]+)(\s*[:=]?\s*)(.*)$` on the line; key = group 1 (for disabled entries the prefix is cut off), value = group 3 stripped; separator `=` or `:` if present in group 2 (with the whitespace before/after it remembered), else a single space (original whitespace, including tabs, is **not** kept).
- File-wide separator style = that of the first active entry (`:111-118`); used for appended lines.

Render (`:29-35`): `entry` -> `key + pre + sep + post + value`, right-stripped; `disabled_entry` -> `#` + the same (always `#`, whatever the original prefix was); comments/blanks verbatim. Save writes every line + `\n` (`:249-252`), opened with `"w"` (ownership kept).

Set (`SC/flatconf.py:143-224`), with `FlatProvider.set` converting first (`SC/provider.py:259-268`): `bool` -> `"true"`/`"false"`, `int` -> `str`, list/tuple -> list of `str`. A string that starts with `[` and is a valid JSON array is treated as a list.
- Matching lines = all `entry` **and** `disabled_entry` lines with that key, in file order.
- None match: append one `entry` per non-empty value at the end of the file (empty values are skipped).
- Some match: the first `n = min(len(values), len(matches))` matching lines are updated in place: non-empty value -> becomes an active `entry` with that value; empty value -> becomes `disabled_entry` with an empty value (renders as `#key`).
- More values than matches: extra entries are inserted right after the last matching line.
- Fewer values than matches: the surplus matching lines (active or commented) are **deleted**.
- Example: stock `mosquitto.conf` has `#listener` (line 234); setting `listener_port="1883 0.0.0.0"` turns it into `listener 1883 0.0.0.0` at the same position.

Get: all values for a key from `entry` and `disabled_entry` lines; one value -> string, several -> list (`SC/provider.py:249-257`). Note: commented-out values are returned too.

Files: `/etc/mosquitto/mosquitto.conf`, `/opt/openthread/otbr-agent.conf`, `/etc/ssh/sshd_config` (4.10).

#### `single` (`SC/provider.py:466-511`)

- Load: whole file, `strip()`. **Missing file raises**, so the provider cannot be created.
- Get/Set ignore the path; set stores the string.
- Save: write `value + "\n"` (nothing if the value is empty), opened with `"w"`. If the path ends with `timezone` and the value is non-empty: remove `/etc/localtime` (file or symlink) and create a symlink `/etc/localtime -> /usr/share/zoneinfo/<value>` (absolute target, no existence check) (`:494-502`).
- Files: `/etc/hostname`, `/etc/timezone`, `/etc/usb_mode`.

### 4.2 Page settings flow and page keys

`POST /api/v1/pages/{key}/settings` with a JSON object (`BE/api/routes/pages.py:154-196`):
1. Empty object -> return `{"status":"ok"}` (no handler).
2. Write every key to `devicesettings` (1.2 encoding). Page row must exist (else 404).
3. Run the page handler `PageHandlerFactory.get_handler(key.lower())` (`BE/page_handlers.py:484-509`) with the **raw JSON body** (handlers may mutate it, after the DB write).
4. Handlers that return false (default handler, Wi-Fi without SSID) -> fallback: `ProviderController(<key>)` -> `bulk_set(body)` -> `save()`, errors only logged. In practice no page has a matching `configsource`, so nothing happens.
5. A handler exception -> 500 (DB already written).

`GET /pages/{key}/settings` returns all keys; if `start_on_boot` exists it is replaced by `str(check_autostart(key)).lower()` (`BE/api/routes/pages.py:132-151`). `PUT /pages/{key}/settings/{k}` (body `{"value": "<str>"}`) and `DELETE ...` only touch the DB, no handler (`:237-316`). `POST /pages` creates a page `{key, name, description}` (400 if it exists) (`:49-68`).

Page keys in use and what the old UI stores (setting keys are the DB keys):

| Page key | Created by | Setting keys stored | Handler (section) |
|---|---|---|---|
| `esphome` | migration `4cc919e10054` | `buzzer`, `ambient_light`, `user_i2c` (bool), `bluetooth_proxy_mode` (`off`/`hci`/`bluez`), `encryption_key` (base64 of 32 bytes); handler also accepts `adc1`, `adc2` (`FE/schemas/esphome.json`) | 5 |
| `general` | UI | `hostname`, `timezone`, `advanced` (bool), `sdcard-config` (bool) (`FE/schemas/general.json`) | 4.4 |
| `usb` | UI | `usb_mode` (`device`/`host`) | 4.5 |
| `lte` | UI | `apn` (stored only), `start_on_boot` (bool), `led_control` (bool; true = LED off) | 4.6 |
| `updates` | UI | `os_update_channel` (`stable`/`development`), `auto_update` (stored only) | 4.7 |
| `mqtt` | UI (schema `mosquitto`, page key `mqtt`, `FE/pages/Settings/Mqtt.jsx:14,78`) | `listener_port` (`1883` or `1883 0.0.0.0`), `allow_anonymous`, `enable_bridge_mode`, `remote_connection`, `remote_address`, `remote_username`, `remote_password`, `bridge_topic` (JSON list or `""`), `clean_session` | 4.8 |
| `ser2net` | UI | `ports` (JSON list of `{"serial_port","baudrate","tcp_port"}`), `start_on_boot` | 4.11 |
| `security` | UI | `enabled` (`"true"`/`"false"`), `email` | 2.4 + 4.10 |
| `wireguard` | UI (schema `wireguard-client`) | `autoconnect`, `addresses` (JSON list of CIDR), `private_key`, `endpoint_address` (`host:port`, the UI appends `endpoint_port` with `:`), `public_key`, `disable_advanced_settings`, `keepalive`, `preshared_key`, `allowed_ips` (JSON list) - keys stored in **plain text** | 4.12 |
| `ethernet` | UI | `ip_method`, `addresses`, `gateway`, `dns`, `mtu`, `auto_connect_on_boot`, `connection` (NM connection name) | 4.13 |
| `wifi` | UI | `ssid`, `passphrase` (plain text), `hidden_network`, `security` (`WPA2`/`WPA3`/`WPA2WPA3`/`WPAMixed`/`WPA`/`None`), `ip_method` (`AUTO`/`STATIC`), `ip_address`, `ipv4_gateway`, `ipv4_dns` | 4.14 |
| `radios` | UI (`PUT` single keys) | `mode<N>`, `version<N>`, `ieee<N>` (all strings) | 6 |
| `backups` | UI | `extra_dirs`, `include_home` (stored only) | none |
| `mosquitto`, `ssh` | handler aliases only (`BE/page_handlers.py:493-495`) | | 4.8 / 4.10 |

How the UI shapes values (needed to read existing rows): booleans are JSON booleans (stored `True`/`False`); a field with `appendTo` is concatenated into its parent with `appendSeparator` (default space) and is not stored itself (`FE/components/Settings/SchemaForm.jsx:278-296`, `:482-485`); a field disabled by an off toggle is sent as `""` (`:471-474`); `text-list` fields are sent as arrays (`:487-490`).

### 4.3 Shared helper for single-value system files

`update_system_single_settings(settings, {setting: source})` (`BE/page_handlers.py:51-78`): for each mapped key with a **truthy** value: `ProviderController(source)` (fails if the file is missing), compare `get_from_string(key)` with the new value, and if different `set_from_string` + `save()`. Errors are logged only.

### 4.4 `general` page (`BE/page_handlers.py:143-165`)

| Key | Effect |
|---|---|
| `hostname` | `/etc/hostname` = `<hostname>\n` (only if different). No `hostname` command, no `/etc/hosts` change, NetworkManager not touched. |
| `timezone` | `/etc/timezone` = `<tz>\n` and symlink `/etc/localtime -> /usr/share/zoneinfo/<tz>` (4.1 `single`). Only if `/etc/timezone` already exists (otherwise the provider cannot load and the change is skipped with an error log) and differs. |
| `sdcard-config` | `true`/`"true"` (case-insensitive) -> `touch /mnt/user/.sdcard-config-enabled`; else delete it (`:81-94`). |
| `advanced` | nothing |

Other timezone/hostname writers: `POST /utils/tz {"timezone": "..."}` only replaces the `/etc/localtime` symlink (validates that `/usr/share/zoneinfo/<tz>` exists; does **not** write `/etc/timezone`) (`SC/tz.py:49-61`, `BE/api/routes/utils.py:276-286`); `GET /utils/tz` returns the part after `/zoneinfo/` of the `/etc/localtime` link target (`SC/tz.py:63-76`); `POST /net/hostname?hostname=<name>` writes `/etc/hostname` through the same provider, no auth (`BE/api/routes/net.py:238-249`).

Time zone list `GET /utils/tz/list`: names from `pytz.common_timezones` (`SC/tz.py:40-47`).

### 4.5 `usb` page (`BE/page_handlers.py:349-367`)

`usb_mode` -> `/etc/usb_mode` = `device\n` or `host\n` (only if different). The OS `usb-watcher` reacts to the file change. The LTE page of the UI switches it to `host` before powering the modem (`FE/pages/Settings/Lte.jsx:95-127`).

### 4.6 `lte` page (`BE/page_handlers.py:168-205`)

1. Always: `Packages.set_autostart("lte", body.get("start_on_boot", False))` - flag `/opt/share/services/lte.autostart` + `rc-update add|del lte default` (there is no `/etc/init.d/lte`, so `rc-update` fails and is ignored; **Inferred**). A payload without `start_on_boot` therefore **disables** autostart. The flag is read by the OS `modem-power` service at boot.
2. If `led_control` present (bool or `"true"`): GPIO `4g_led` (4g_led = chip `porte` offset 4, active-high) set to `0` when `led_control` is true (LED off) and `1` otherwise.
3. `apn` is not used anywhere.

Startup counterpart: when the modem controller starts and `lte.led_control` is `"true"`, the backend sets `4g_led` to **1** (`BE/modem_controller.py:839-860`) - the opposite of the page handler (bug list).

### 4.7 `updates` page (`BE/page_handlers.py:259-276`, `SC/packages.py:750-774`)

- `channel = body.get("os_update_channel", "stable")`; `development` -> `dev`.
- File `/etc/opkg/smlight.conf` must exist (else error -> 500). Read lines, **drop empty lines** and every line whose stripped text starts with `src/gz smhub_dev`; if channel is `dev` append `src/gz smhub_dev https://pkg.smlight.tech/dev`; write lines joined with `\n` plus a final `\n` (empty file if no lines).
- The file also holds an `option http_auth <user>:<password>` line with the feed credentials (shipped in `OS/etc/opkg/smlight.conf`; value not reproduced) and `src/gz smhub_core ...`, `src/gz community ...` lines; they are kept as they are.
- Example result with `development`:
  ```
  option http_auth <feed-credentials>
  src/gz smhub_core https://pkg.smlight.tech/v1
  src/gz community https://pkg.smlight.tech/community
  src/gz smhub_dev https://pkg.smlight.tech/dev
  ```
  (lines other than the last are whatever was in the file).
- No `opkg update` is run. `auto_update` is not used by the backend.

### 4.8 `mqtt` / `mosquitto` page (`BE/page_handlers.py:208-256`)

1. Convert boolean values of the body to `"true"`/`"false"` (the DB already has `True`/`False`).
2. `merged = {persistence_file: "mosquitto.db", persistence_location: "/var/lib/mosquitto/", log_dest: "file /var/log/mosquitto.log", password_file: "/var/lib/mosquitto/passwd", include_dir: "/etc/mosquitto/conf.d"}` updated with the body (body wins). `include_dir` has no `configsetting`, so it is ignored.
3. `ProviderController("mosquitto")` (flat, comment prefix `#`) -> `bulk_set(merged)` -> `save()` on `/etc/mosquitto/mosquitto.conf`, mapping names to directives via 1.6: `listener_port`->`listener`, `bind_interface`, `persistence_file`, `persistence_location`, `log_dest`, `password_file`, `allow_anonymous`, `remote_connection`->`connection`, `remote_address`->`address`, `remote_username`, `remote_password`, `clean_session`->`cleansession`, `bridge_topic`->`topic`. `enable_bridge_mode`, `allow_external` are not file keys.
4. Restart: `rc-service mosquitto restart` (OpenRC), else `/etc/init.d/S50mosquitto restart`; errors logged only.

How `allow_external` reaches the file: the UI field `allow_external` has `appendTo: listener_port, appendValue: "0.0.0.0"` (`FE/schemas/mosquitto.json:18-29`), so the UI sends `listener_port: "1883 0.0.0.0"` when on and `listener_port: 1883` when off. The directive becomes `listener 1883 0.0.0.0` or `listener 1883`. When reading back, the UI sets `allow_external` = (`listener_port` contains `0.0.0.0`) (`FE/components/Settings/SchemaForm.jsx:182-190`).

Bridge: with the toggle off the UI sends `""` for `remote_*`, `bridge_topic`, `clean_session`, which comments the directives out (`#connection`, `#address`, `#topic`, `#cleansession`, `#remote_username`, `#remote_password`). With it on: `connection <name>`, `address <host:port>`, one `topic <spec>` line per list element (in place of the first `#topic`, further ones inserted after it), `remote_username`, `remote_password` (plain text), `cleansession true|false`.

Resulting file for the stock mosquitto.conf (only changed lines shown, in file order; line numbers are those of the stock file `fastapi-backend/rootfs/etc/mosquitto/mosquitto.conf`, assumed equal to the device's - **Inferred**):

```
listener 1883 0.0.0.0                                  # was "#listener"            (line 234)
persistence_file mosquitto.db                          # was "#persistence_file mosquitto.db" (433)
persistence_location /var/lib/mosquitto/               # was "#persistence_location" (439)
log_dest file /var/log/mosquitto.log                   # was "#log_dest stderr"      (472)
allow_anonymous false                                  # was "#allow_anonymous false" (533)
password_file /var/lib/mosquitto/passwd                # was "#password_file"       (551)
connection mybridge                                    # was "#connection <name>"    (688)  bridge on
address mqtt.example.com:1883                          # was "#address <host>..."    (689)
topic # both 1                                         # was "#topic <topic> ..."    (690)
topic home/# out 0                                     # inserted after it
cleansession true                                      # was "#cleansession false"   (717)
remote_password <secret>                               # (757)
remote_username <user>                                 # (763)
```
(The `# was` remarks are explanations, not file content.)

### 4.9 Authorized SSH keys - see section 16.

### 4.10 `security` / `ssh` page (`BE/page_handlers.py:463-473`, `BE/sshd.py:61-81`)

1. Load `/etc/ssh/sshd_config` with the flat provider (comment prefix `#`).
2. For each of `PermitRootLogin`, `PubkeyAuthentication`, `PasswordAuthentication` present in the body: `set(directive, str(value))` (so JSON `true` becomes `True`; the UI never sends these keys).
3. **Always** save the file and run `rc-service sshd restart` (failure -> 500).

Effect of every save of the `security` page today: `sshd_config` rewritten through the flat editor, which replaces the whitespace between key and value by one space (e.g. `AuthorizedKeysFile<TAB>/mnt/misc/...` -> `AuthorizedKeysFile /mnt/misc/...`, `Subsystem<TAB>sftp<TAB>...` -> `Subsystem sftp<TAB>...`) and right-strips lines; then sshd restarts.

### 4.11 `ser2net` page (`BE/page_handlers.py:433-460`, `BE/ser2net.py`)

Parse `body["ports"]` (list; non-dict items and items without `serial_port` skipped) (`BE/ser2net.py:27-45`):
- name = basename of `serial_port` (e.g. `ttyS1`, `ttyUSB0`; a later port with the same basename overwrites),
- `baud = int(baudrate or 115200)`, `tcp = int(tcp_port) if tcp_port else 2000`,
- connection `{"accepter": "tcp,<tcp>", "connector": "serialdev,<serial_port>,<baud>n81,local"}`.

No connections -> cleanup (`:47-69`): delete `/etc/ser2net/ser2net.yaml` if present, `rc-service ser2net stop` (errors logged), `Packages.set_autostart("ser2net", False)` (flag file removed, `rc-update del ser2net default`).

Otherwise (`:71-105`):
1. If the file does not exist: create `/etc/ser2net/` and write `%YAML 1.1\n---\n`.
2. Load it with the yaml provider, drop a legacy top-level `connection` key, set `connections` to the new map (replacing the old map completely), save with header `%YAML 1.1\n` (4.1).
3. `rc-service ser2net restart` (errors logged).
4. `Packages.set_autostart("ser2net", body.get("start_on_boot", False))`.

Expected file (keys sorted; test `BE/tests/test_page_handlers.py:155-177` checks these lines):

```yaml
%YAML 1.1
---
connections:
  ttyS1:
    accepter: tcp,5001
    connector: serialdev,/dev/ttyS1,115200n81,local
  ttyUSB0:
    accepter: tcp,6002
    connector: serialdev,/dev/ttyUSB0,9600n81,local
```
Other top-level keys already in the file are kept (sorted).

The hub's ser2net refuses this file ("Invalid token at the main level:
connections") and opens no port, while OpenRC counts it as started; 2.0
writes ser2net's `connection` entries instead (D58).

`GET /utils/os/services/status` includes `ser2net` only if `/etc/ser2net/ser2net.yaml` exists (`BE/api/routes/utils.py:304-305`).

### 4.12 `wireguard` page -> NetworkManager (`BE/page_handlers.py:279-310`, `SC/nm_providers.py:184-318`)

The handler adds `name: "wg0"` to the body (after the DB write) and uses `WireguardProvider("wg0")` (connection name `wg0`, interface `wg0`). If a connection named `wg0` exists -> D-Bus `Settings.Connection.Update(<full settings>)`, else `Settings.AddConnection(<settings>)`. It does **not** activate the connection. Validation errors (missing `private_key`, `addresses` or `public_key`) are returned as `{"success": false}` and ignored (the handler still returns success).

Settings built (`SC/nm_providers.py:217-318`):

| Setting | Value |
|---|---|
| `connection.id` / `type` / `interface-name` | `wg0` / `wireguard` / `wg0` |
| `connection.autoconnect` | `body.autoconnect` (default true) |
| `wireguard.private-key` | `private_key` |
| `ipv4.method` + `address-data` | `manual` with every address without `:` (`address`, `prefix` from `a.b.c.d/nn`); `disabled` if none |
| `ipv6.method` + `address-data` | `manual` with every address containing `:`; `disabled` if none |
| peer `public-key` | `public_key` |
| peer `allowed-ips` | `allowed_ips` if the key is present (empty list = no allowed IPs); if absent: for each address its /24 (IPv4) or /64 (IPv6) network |
| peer `endpoint` | `endpoint_address` if non-empty (already `host:port` from the UI) |
| peer `preshared-key` | `preshared_key` if non-empty |
| peer `persistent-keepalive` | `keepalive` if not null (uint32) |
| DNS | none (WireGuard DNS is not supported by the old code) |

`Update` replaces the whole connection (anything not listed falls back to NM defaults); the UUID stays.

`nmcli` equivalent (**Inferred**; `nmcli` has no syntax to edit WireGuard peers on most NM versions, so use import):
```sh
# build a wg-quick style file named exactly wg0.conf (the connection takes the file name)
cat > /run/smhub/wg0.conf <<'EOF'
[Interface]
PrivateKey = <private_key>
Address = 10.0.0.2/32
[Peer]
PublicKey = <public_key>
AllowedIPs = 10.0.0.0/24           # allowed_ips, or the derived /24 and /64 networks
Endpoint = vpn.example.com:51820   # only if endpoint_address is set
PresharedKey = <preshared_key>     # only if set
PersistentKeepalive = 25           # only if keepalive is set
EOF
nmcli connection delete id wg0 2>/dev/null          # update = delete + import (UUID changes)
nmcli connection import type wireguard file /run/smhub/wg0.conf
nmcli connection modify wg0 connection.autoconnect <yes|no> ipv6.method disabled   # ipv6 "disabled" when no IPv6 address
```
If keeping the UUID matters, use the D-Bus `Update` call as today (e.g. with `zbus`) instead of `nmcli`.

### 4.13 `ethernet` page -> NetworkManager (`BE/page_handlers.py:114-140`, `SC/nm_providers.py:834-912`)

- Connection name = `body.connection` (the UI sends the active connection name of the Ethernet device, or the device name) or `eth0`; interface `eth0`.
- Exists -> `Update(<full settings>)`, else `AddConnection`. If the result is success -> `ActivateConnection(<conn path>, "/", "/")` (= `nmcli connection up id <name>`).
- Keys read by the provider: `autoconnect` (default true), `ip_method` (`auto`/`manual`/`disabled`/`link-local`, default `auto`; validated), `addresses` (list of CIDR, required for manual), `gateway`, `dns_servers` (list), `mtu`, `mac_address`, `cloned_mac_address`. The UI sends `dns` and `auto_connect_on_boot`, which are **ignored** (so DNS from the UI is never applied and autoconnect is always true), and `mtu: 1500`.

Settings built:

| Setting | Value |
|---|---|
| `connection.id` / `type` / `interface-name` / `autoconnect` | name / `802-3-ethernet` / `eth0` / true |
| `802-3-ethernet.mtu` | `mtu` if truthy |
| `802-3-ethernet.mac-address`, `cloned-mac-address` | if given |
| `ipv4` | `build_ip_settings` below; also an `ipv4.mtu` key when `mtu > 0` (not a real NM property; **Inferred** ignored by NM) |
| `ipv6` | `build_ip_settings` below |

`build_ip_settings(method, addresses, gateway, dns_servers, mtu)` (`SC/nm_controller.py:594-682`):
- `ipv4.method = <method>`; `ipv6.method = auto`.
- Only when method is `manual` and addresses are given: IPv4 addresses -> `ipv4.address-data`; IPv6 addresses -> `ipv6.method = manual` + `ipv6.address-data`; `gateway` -> `ipv4.gateway` or `ipv6.gateway` by family; DNS IPv4 -> `ipv4.dns` as `au` of `int(IPv4Address)` (host-order integer, which NM reads as network order on little-endian RISC-V, so the address is byte-reversed - bug list), IPv6 -> `ipv6.dns` as `aay` of packed bytes.

`nmcli` equivalent:
```sh
# create (connection did not exist)
nmcli connection add type ethernet con-name "<name>" ifname eth0 connection.autoconnect yes \
  ipv4.method auto ipv6.method auto 802-3-ethernet.mtu 1500
# manual IPv4
nmcli connection add type ethernet con-name "<name>" ifname eth0 connection.autoconnect yes \
  ipv4.method manual ipv4.addresses "192.168.1.10/24" ipv4.gateway 192.168.1.1 \
  ipv4.dns "1.1.1.1 8.8.8.8" ipv6.method auto
# update: Update() replaced everything, so reset what is not given:
nmcli connection modify "<name>" connection.interface-name eth0 connection.autoconnect yes \
  ipv4.method auto ipv4.addresses "" ipv4.gateway "" ipv4.dns "" ipv6.method auto ipv6.addresses "" ipv6.gateway "" ipv6.dns ""
nmcli connection up id "<name>"
```

### 4.14 `wifi` page -> NetworkManager (`BE/page_handlers.py:313-346`, `SC/nm_providers.py:321-469`)

- `ssid = body.ssid` (missing -> handler returns false, fallback does nothing); `body.password = body.passphrase`.
- Connection name = the SSID; interface `wlan0` is only used for device lookups, the connection gets **no** `interface-name`.
- Exists -> `Update`, else `AddConnection`. On success: `deactivate_active_wifi()` - if any Wi-Fi AP is active, deactivate the active connection **named like the new SSID** (if any) - then `ActivateConnection(conn, "/", "/")`.
- Validation (`:324-371`): `security` (default `wpa2-psk`) must be one of `none, wep, wpa-psk, wpa2-psk, wpa3-psk, wpa-psk+WPA2-psk, wpa2-psk+WPA3-psk, sae`; password required unless `none`; `ip_method` in `auto, manual, disabled, link-local`; `addresses` required for manual. **The 1.x UI sends `security` = `WPA2`/`WPA3`/`WPA2WPA3`/`WPAMixed`/`WPA`/`None` and `ip_method` = `AUTO`/`STATIC`, so validation fails and no connection is created or changed today** (bug list). The UI's `hidden_network`, `ip_address`, `ipv4_gateway`, `ipv4_dns` are not read (provider keys are `hidden`, `addresses`, `gateway`, `dns_servers`).

Settings built (`:373-469`):

| `security` | `802-11-wireless-security` |
|---|---|
| `none` | (section absent; `802-11-wireless.security` absent) |
| `wep` | `key-mgmt=none`, `wep-key0=<pw>`, `wep-key-type=1` (NM: 1 = hex/ASCII key, not passphrase), `auth-alg=open` |
| `wpa-psk` | `key-mgmt=wpa-psk`, `psk=<pw>`, `proto=[wpa]` |
| `wpa2-psk` | `key-mgmt=wpa-psk`, `psk=<pw>`, `proto=[rsn]` |
| `wpa3-psk`, `sae` | `key-mgmt=sae`, `psk=<pw>` |
| `wpa-psk+WPA2-psk` | `key-mgmt=wpa-psk`, `psk=<pw>`, `proto=[wpa, rsn]` |
| `wpa2-psk+WPA3-psk` | `key-mgmt="wpa-psk sae"` (not a valid single NM value; **Inferred** rejected), `proto=[rsn]` |

Plus `connection.id=<ssid>`, `type=802-11-wireless`, `autoconnect=<autoconnect, default true>`, `802-11-wireless.ssid=<utf-8 bytes>`, `mode=infrastructure`, `hidden=true` if `hidden`, `802-11-wireless.security=802-11-wireless-security` when secured, and `ipv4`/`ipv6` from `build_ip_settings` (4.13, without mtu).

`nmcli` equivalent:
```sh
nmcli connection add type wifi con-name "<ssid>" ssid "<ssid>" 802-11-wireless.mode infrastructure \
  connection.autoconnect yes [802-11-wireless.hidden yes] \
  wifi-sec.key-mgmt wpa-psk wifi-sec.psk "<password>" wifi-sec.proto rsn \
  ipv4.method auto ipv6.method auto
nmcli connection up id "<ssid>"
```
(NM versions that require `ifname` for `add` accept `ifname "*"`; **Inferred**.)

### 4.15 Other NetworkManager operations (`/api/v1/net/*`, no auth)

All by **connection name** (`id`), D-Bus `org.freedesktop.NetworkManager` (`SC/nm_controller.py`). A name lookup lists all connections and compares `connection.id` (`:280-292`).

| Old endpoint | D-Bus action | `nmcli` equivalent | Response shape (old) |
|---|---|---|---|
| `GET /net/devices/list` | for each device: Interface, DeviceType, State, Driver, HwAddress, IpInterface, ActiveConnection->id (`:181-253`) | `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device` + `nmcli -t -f GENERAL.DRIVER,GENERAL.HWADDR,GENERAL.IP-IFACE device show <dev>` | `{"devices":[{interface,type,type_code,state,state_code,driver,hw_address,ip_interface,active_connection,path}]}`; type codes 1 ethernet, 2 wifi, 5 bluetooth, 13 bridge, 14 generic, 20 veth, 29 wireguard, else `unknown-<n>`; states 0 unknown,10 unmanaged,20 unavailable,30 disconnected,40 prepare,50 config,60 need-auth,70 ip-config,80 ip-check,90 secondaries,100 activated,110 deactivating,120 failed (`:548-580`) |
| `GET /net/connections/list` | `ListConnections` + `GetSettings` (`:156-179`) | `nmcli -t -f NAME,TYPE,DEVICE,AUTOCONNECT connection show` | `{"connections":[{name,type(e.g. 802-3-ethernet),interface,autoconnect,path}]}` |
| `GET /net/connections/{name}` | `GetSettings` (`:260-269`, `:395-427`) | `nmcli connection show id <name>` | `{"settings":{name,type,autoconnect,ipv4:{method,addresses:[{address,prefix}]},ipv6:{...}}}` or null |
| `GET /net/connections/{name}/active` | active connection: State, Default, Default6, Ip4Config/Ip6Config AddressData, Gateway, Nameservers (`:429-518`) | `nmcli -t connection show --active id <name>` (`GENERAL.STATE`, `IP4.ADDRESS`, `IP4.GATEWAY`, `IP4.DNS`, ...) | `{"settings":{state,state_name (0 unknown,1 activating,2 activated,3 deactivating,4 deactivated),default,default6,ipv4:{addresses,gateway,nameservers},ipv6:{...}}}` or null |
| `GET /net/connections/{name}/up` | `ActivateConnection(path,"/","/")` (`:343-364`) | `nmcli connection up id <name>` | message |
| `GET /net/connections/{name}/down` | `DeactivateConnection` of the active connection with that id (`:366-393`) | `nmcli connection down id <name>` | message |
| `DELETE /net/connections/{name}` | `Connection.Delete` (`:320-341`) | `nmcli connection delete id <name>` | message |
| `GET /net/wifi/scan?rescan=false` (or `true`) | Wi-Fi device (type 2), `RequestScan({})` + sleep 2 s if rescan, `GetAllAccessPoints` (`SC/nm_providers.py:636-777`) | `nmcli -t -f SSID,BSSID,FREQ,SIGNAL,WPA-FLAGS,RSN-FLAGS,SECURITY device wifi list ifname <dev> --rescan yes` (or `no`) | `{"networks":{success,networks:[{ssid,signal_strength(0-100 %),frequency,security,hw_address(BSSID),channel}],count}}`; APs with empty SSID skipped, sorted by strength desc, duplicates kept |
| `GET /net/wifi/active` | active AP of the Wi-Fi device (`:471-594`) | `nmcli -t -f ACTIVE,SSID,BSSID,FREQ,SIGNAL,RATE device wifi list` | `{"wifi_info":{success,ssid,signal_strength,frequency,channel,security,hw_address,max_bitrate (kb/s)}}` (connection info is looked up under the name `wifi_active`, so it is never added) |
| `GET /net/wifi/down` | deactivate active connection named `wifi_down` (`BE/api/routes/net.py:163-180`) | `nmcli connection down id "<active wifi connection>"` is what was intended | always fails today (bug list) |
| `GET /net/isListening?url=` | TCP connect to host:port (default 80), 1 s timeout (`BE/utils.py:10-32`) | | bool |
| `GET /net/hostname` | `/etc/hostname` via provider | | string |

Security label of an AP (`SC/nm_providers.py:779-809`): RSN flags non-zero -> `WPA3` if bit 0x400 (SAE) set (or `WPA2/WPA3` if WPA flags also non-zero), else `WPA2`; else WPA flags non-zero -> `WPA`; else privacy flag (0x1) -> `WEP`; else `Open`. Channel (`:811-831`): 2412..2484 MHz -> `(f-2412)/5+1` (2484 -> 14); 5170..5825 -> `(f-5170)/5+34`; else 0.

IPv4 nameservers from the deprecated `Nameservers` (`au`) property are converted with little-endian byte order (`SC/nm_controller.py:526-546`).

NetworkManager monitoring (`SC/nm_monitor.py`): subscribes to NM `StateChanged` and `PropertiesChanged` of active connections; for every state change of any active connection it broadcasts on the shared WebSocket `{"type":"nm_status","timestamp":"<local ISO>","connection":"<name>","data":{state,state_name,ip4_config:{path,gateway,addresses,nameservers},ip6_config:{...},vpn_state,default,default6}}` (`BE/main.py:125-142`, `SC/nm_monitor.py:138-258`). No files.

---

## 5. ESPHome / RTOS coprocessor (`BE/rtos.py`)

Files and constants (`BE/rtos.py:14-31`):

| Item | Value |
|---|---|
| Patched ELF (loaded by the OS `remoteproc` service) | `/opt/firmware/smhub-rtos.elf` |
| Unpatched backup ELF (from the `esphome-bin` package) | `/var/lib/opkg/info/esphome-bin.nano.elf` if the EEPROM model contains `nano` (case-insensitive), else `/var/lib/opkg/info/esphome-bin.smhub.elf` (`:38-47`) |
| Overlay source dir | `/lib/firmware/overlays` |
| Overlay destination dir | `/mnt/misc/overlays` |
| Bluetooth proxy mode file | `/opt/firmware/bluetooth_proxy_mode` |
| Factory-default ESPHome API encryption key | 32 bytes, the factory-default key bytes in `BE/rtos.py:16` (base64 constant, decoded at `:17`). The same constant is in migration `4cc919e10054` (`:53`), the seed DB and `FE/schemas/esphome.json`. Not reproduced here. |

### 5.1 Overlays (`configure_overlays`, `:49-88`)

| Setting | Overlay file |
|---|---|
| `ambient_light` | `rtos-spi0.dtbo` |
| `buzzer` | `rtos-pwm3.dtbo` |
| `user_i2c` | `rtos-i2c4.dtbo` |
| `adc1` | `rtos-adc1.dtbo` |
| `adc2` | `rtos-adc2.dtbo` |

Only keys present in the settings are processed. Enabled: if `/mnt/misc/overlays/<file>` does not exist and `/lib/firmware/overlays/<file>` exists -> create the dest dir and `shutil.copy2` (copies mode and mtime; owner root). Existing dest files are not overwritten. Disabled: delete the dest file if present. Nothing touches `/mnt/misc/uEnv.txt` (no code references it). The OS `dtbo-watcher` reacts to changes in `/mnt/misc/overlays` (`OS/etc/init.d/dtbo-watcher:3`).

Boolean conversion differs by caller: the page handler uses Python `bool(value)` on the raw JSON value (`BE/page_handlers.py:385-396`), the startup sync first maps strings `true`/`false` (case-insensitive) to booleans (`BE/rtos.py:228-236`). A string `"False"` sent over HTTP would therefore count as enabled.

### 5.2 Encryption key patch (`patch_encryption_key`, `:130-219`)

Input: base64 string; must decode to exactly 32 bytes (else `ValueError`; the page handler logs it and continues).
1. If the patched ELF is missing: copy the backup ELF to it (`copy2`, creating `/opt/firmware`); if there is no backup -> return "not patched".
2. If the new key bytes already occur anywhere in the patched ELF -> return "not patched" (nothing to do).
3. If the backup ELF is missing -> return "not patched".
4. Read the backup ELF. If the new key equals the factory key -> write the backup bytes unchanged to the patched ELF, return "patched".
5. Else find the **first** occurrence of the 32 factory-key bytes in the backup (`bytes.find`); not found -> `ValueError`. Replace those 32 bytes with the new key, write the whole buffer to `/opt/firmware/smhub-rtos.elf` (`write_bytes`, in place, no temp file), return "patched".

### 5.3 Bluetooth proxy mode (`update_bluetooth_proxy_mode`, `:90-128`)

`mode.strip()` is compared with the stripped content of `/opt/firmware/bluetooth_proxy_mode`; if the file is missing or different, write the mode **without newline** (creating the dir). Then `rc-service smhub-broker restart` if it changed or if `force_restart` (= the key was patched). Values from the UI: `off`, `hci`, `bluez`.

### 5.4 Page handler and startup sync

`esphome` page handler (`BE/page_handlers.py:370-430`) and `sync_settings_on_startup` (`BE/rtos.py:221-287`, run at every backend start from `devicesettings` of page `esphome`) do the same:
1. Overlays for the present keys.
2. If `encryption_key` is non-empty: patch (errors logged).
3. If `bluetooth_proxy_mode` is present: update it (restart smhub-broker if changed or patched).
4. Else if patched: `rc-service smhub-broker restart`.

`POST /api/v1/utils/os/rtos-restart` (no auth) = `rc-service smhub-broker restart` (`BE/api/routes/utils.py:87-100`). The `rtos-notify` command entry in `SC/data.py:130-135` is never used by the backend.

---

## 6. Radios (`BE/api/routes/radio.py`, `BE/api/routes/radio_patch.py`, `SC/firmware.py`)

All `/api/v1/radio/*` routes have **no auth**. The router is optional (import errors only log a warning, `BE/api/main.py:50-58`).

### 6.1 Device maps

Model = lowercased EEPROM model (fallback `essential` with hw `0.96` when there is no EEPROM, `BE/utils.py:35-46`). Index N is the radio, serial port `/dev/ttyS<N+1>` (`BE/api/routes/radio.py:376`, `:503`).

| Model | idx 0 | idx 1 | idx 2 |
|---|---|---|---|
| `essential` | SMHUB-CCP1 | SMHUB-MG21 | SMHUB-ZG23 |
| `premium` | SMHUB-CCP10 | SMHUB-MG24 | SMHUB-ZG23 |
| `nanop1` / `nanop7` / `nanop10` | CCP1 / CCP7 / CCP10 | | |
| `nanomg21` / `nanomg24` / `nanomg26` | MG21 / MG24 / MG26 | | |
| `nanomr1` | CCP7 | MG21 | |
| `nanomr2` | CCP1 | MG21 | |
| `nanomr3` | CCP10 | MG24 | |
| `nanomr4` | CCP10 | MG26 | |
| `nanomr5` | MG24 | MG24 | |
| `nanomr6` | CCP7 | MG24 | |
| `nanomr7` | CCP7 | MG26 | |

(`BE/api/routes/radio.py:63-120`.) `GET /radio/radio_config` -> `{"config": {<idx>: "<device>"}}` or `{"config": null}`.

### 6.2 Firmware list

`GET /radio/{idx}/firmware_list?type=<int>` -> `get_radio_firmware_version(channel="dev", mode="zigbee", zb_type=type, device=<device>)` (`BE/api/routes/radio.py:252-291`, `SC/firmware.py:38-108`):
- Request: `GET https://updates.smlight.tech/services/api/slzb-06x-ota.php?type=ZB&format=slzb&device=<id>` with `User-Agent: system-config`, timeout 10 s (`SC/http_client.py:46-50`). Device ids: MG21 65, CCP1 66, MG24 67, MG26 68, CCP7 69, CCP10 70, ZG23 71, MG301 72 (`SC/firmware.py:13-22`).
- Response: JSON array of objects. Per item: `prod` (default true), `dev` (default `not prod`), version from `ver` or `rev` or `version` (items without one are skipped), `type` (items whose int type differs from the requested `type` are skipped), link from `link` or `url`, `notes`.
- Because channel is always `dev`, dev items are included; their `notes` get the prefix `[dev] ` (or become `[dev]`) unless already prefixed (`BE/api/routes/radio.py:278-287`).
- Result items `{ver, link, dev, prod, notes, type, mode:"zigbee"}`; cached per `"<idx>_<type>"` in memory. Empty list -> 500.

### 6.3 Flash procedure

`POST /radio/{idx}/flash_firmware?type=<int>&version=<ver>&ieee=<optional>` (`:338-378`): find the version in the cached (or freshly fetched) list; 404 if missing; 500 if no link. Starts a background task and returns `{"status":"started","port":"/dev/ttyS<idx+1>","version":...}` immediately.

Background task (`:494-567`):
1. Broadcast `{"event":"radio_flash_status","status":"downloading","message":"Downloading firmware for radio <idx>","version","port"}`.
2. Download the whole file into memory (`HTTPClient`, 10 s timeout).
3. Find the OpenRC service that holds the tty (`SC/initManager.py:262-355`): scan `/proc/<pid>/fd/*` for a link equal to the port path; take that PID and up to 10 ancestors (stopping at PID 0/1); look in `/run/openrc/daemons/*/*` for a `pidfile=` line whose pidfile content is one of those PIDs (service = the directory name); else `/run/*.pid` files (service = file stem).
4. If a service was found: `service_context([svc])` = if `rc-status` says `started`, `rc-service <svc> stop`; sleep 2 s; flash; sleep 2 s; `rc-service <svc> start` (only services that were started) (`SC/initManager.py:357-408`).
5. Flash function by device name: contains `MG` -> EFR32 (6.4); `CC` -> CC (6.5); `ZG` -> Z-Wave (6.4 with target ADDON).
6. Broadcast `radio_flash_status` `success` (`"Successfully flashed radio <idx>"`) or `error` (`"Failed to flash radio <idx>: <exc>"`).

Progress (`:294-317`): `{"event":"radio_flash_progress","progress":<int %>,"offset":<int>,"size":<int>}`, only when the integer percentage changes; also logged to `/var/log/universal_silabs_flasher.log` (logger `universal_silabs_flasher`, DEBUG, file handler, `:22-37`).

DB: the backend stores nothing. The UI stores, after a `success` event, `PUT /pages/radios/settings/mode<N>` = `String(mode)` (0 = Zigbee, `ieee` only used then), `version<N>` = version string, `ieee<N>` = IEEE or `""` (`FE/pages/Settings/Radio.jsx:172-181`), and reads them back to show the installed version (`:77-103`).

### 6.4 Silicon Labs (MG*, ZG*) via `universal-silabs-flasher`

Reset targets are injected into the library's `ResetTarget` enum and `RESET_CONFIGS` (`BE/api/routes/radio.py:147-229`). Each pattern step sets the listed lines (True = high) and then waits `delay_after` seconds:

| Target | Used for | Chip(s) | Pattern (line: level ... delay) |
|---|---|---|---|
| `SMHUB` | MG on essential/premium | `/dev/gpiochip3` | `{7:1, 6:1}` 0.1 s -> `{7:0, 6:0}` 0.1 s -> `{7:1, 6:0}` 0.1 s -> `{7:1, 6:1}` 0.5 s |
| `NANO` | MG on `nano*` (and `nanomr4` with hw `0.97`) | `/dev/gpiochip2` | lines 11/12, same pattern with 11 in the place of 7 and 12 in the place of 6 |
| `NANOMR` | MG on other `nanomr*` | line 0 on `/dev/gpiochip1`, line 11 on `/dev/gpiochip3` | `{0:1, 11:1}` 0.1 -> `{0:0, 11:0}` 0.1 -> `{0:1, 11:0}` 0.1 -> `{0:1, 11:1}` 0.5 |
| `ADDON` | ZG23 (Z-Wave) | `/dev/gpiochip3` | `{1:1, 0:1}` 0.1 -> `{1:0, 0:0}` 0.1 -> `{1:1, 0:0}` 0.1 -> `{1:1, 0:1}` 0.5 |

**Inferred** meaning: the first line is the radio's RESET (pulsed low, released first) and the second is its BOOT/bootloader-select line (held low while reset is released). Chip paths are fixed and do not follow the OS-version GPIO chip mapping of section 13.

Multi-chip support (`BE/api/routes/radio_patch.py:8-81`): when the chip is a list, line `i` of the first pattern step goes to chip `i`; lines are requested as outputs with consumer `universal-silabs-flasher` and initial values of step 0; after the last step the lines are reconfigured as inputs and released.

Flash (`BE/api/routes/radio.py:381-442`): `Flasher(probe_methods=((GECKO_BOOTLOADER, 115200),), device=<port>, bootloader_baudrate=115200, bootloader_reset=(<target>,))`; `enter_bootloader()`; `flash_firmware(GBLImage.from_bytes(data), progress_callback, run_firmware=True)`; sleep 1 s; if an IEEE was given (Zigbee only, not for Z-Wave): convert with `zigpy.types.EUI64.convert(ieee)`, new `Flasher(probe_methods=((EZSP, 115200),), device=<port>)`, `write_emberznet_eui64(new_ieee=..., force=True)`; IEEE errors are only logged.

### 6.5 Texas Instruments (CC*) via `smlight_cc_flasher`

`CCFlasher(device=<port>, m33=("P10" in device name))`; `async_init(buffer=<data>, bootloader_mode="smhub", ieee_address=<ieee or None>)`; `connect()`; `flash(progress_callback)` (`BE/api/routes/radio.py:464-482`). The reset/BSL GPIO sequence for `bootloader_mode="smhub"` lives inside the external `smlight_cc_flasher` package (dependency `smlight-cc-flasher>=0.1.6`, `fastapi-backend/backend/pyproject.toml`), not in these repos.

---

## 7. OS update (RAUC)

### 7.1 Update list

`GET /api/v1/utils/os/update-info?channel=stable|dev|...` (no auth) -> `OS_Info().get_os_updates(channel)` (`BE/api/routes/utils.py:121-126`, `SC/os_info.py:109-170`):
- Channels checked: `["stable", "dev"]` if channel is `dev`, else `[channel]`.
- Manifest: `GET https://updates.smlight.tech/firmware/smhub/os/<ch>/manifest.json` (UA `system-config`, 10 s). Format: JSON array of `{"version": "...", "bundle": "<file name>", "changelog": "<file name, optional>", "channel": "<optional>"}`.
- For each entry: `bundle_url = https://updates.smlight.tech/firmware/smhub/os/<ch>/<bundle>`; if `changelog` is set, fetch `.../<ch>/<changelog>` as text (failures -> null).
- Result list of `{version, channel (entry's channel or <ch>), bundle_url, changelog}`, sorted by version descending using PEP 440 parsing with fallbacks: strip a trailing `.N` after a pre-release tag (`1.0.0.beta2.3` -> `1.0.0.beta2`, `1.0.0b2.3` -> `1.0.0b2`), else the leading `X.Y.Z`, else `0.0.0` (`SC/os_info.py:81-107`). A failing channel is skipped. No entries -> `null`.

The UI's `updates` page stores `stable` or `development` and queries `channel=dev` when the stored value starts with `dev` (`FE/pages/Settings/Updates.jsx:51`, `FE/api/index.js:117-118`).

### 7.2 Install bundle

`POST /api/v1/utils/os/install-bundle` (no auth), body `{"source": "<URL or path>", "args": {<optional>}}` (`BE/api/routes/utils.py:168-217`):
- Concurrency guard: a module-global flag; a second call while one runs -> `409 "RAUC installation is already in progress."`. Response `{"message": "RAUC installation started"}`; work runs as a FastAPI background task.
- D-Bus (system bus): service `de.pengutronix.rauc`, object `/`, interface `de.pengutronix.rauc.Installer`, method `InstallBundle(s source, a{sv} args)` where every arg value is sent as a string variant (`SC/rauc_controller.py:15-17`, `:190-223`). Equivalent CLI (**Inferred**, same D-Bus call): `rauc install <source>` (or `busctl call de.pengutronix.rauc / de.pengutronix.rauc.Installer InstallBundle 'sa{sv}' <source> 0`).
- Progress: `PropertiesChanged` of the Installer interface. `Progress` = `(percentage, message, nesting_depth)`; `Operation` alone gives message `Operation: <op>` (not for `idle`) (`SC/rauc_controller.py:115-188`). A synthetic first event is sent with `percentage 0` and message `Starting RAUC installation from: <source>`.
- WebSocket events (shared channel) (`BE/api/routes/utils.py:129-165`):
  - `{"event":"rauc_progress","data":{"percent":<int>,"message":"<str>","depth":<int>}}`
  - `{"event":"rauc_complete","data":{"success":<bool>,"message":"","result":<"0"/"<code>" or null>}}` - from the `Completed(i)` signal (result 0 = success; on failure `LastError` is appended to an `error` field that is **not** forwarded, so `message` is always empty) or from an exception/D-Bus error (`success:false, result:null`).
  - `{"event":"rauc_error","data":{"message":"<exception>"}}` only when the task itself raises.
- The task waits for completion before clearing the guard. No reboot is triggered by the backend.

---

## 8. Backups (`BE/api/routes/stream.py`, prefix `/api/v1/stream`, auth required)

### 8.1 Create (`POST /stream/create-backup`)

Body (optional) `{"extra_dirs": ["/abs/path", ...], "exclude_home": false}` (`:38-47`).

Directories (`:66-97`, `:118-128`):
1. Set = `{"/home/smlight"}` + `system.data_dir.default` (string or each list element) of every package listed as installed in `/var/lib/opkg/status` that has a schema.
2. Sorted, then only existing directories kept.
3. If `exclude_home`, remove `/home/smlight`.
4. Append every `extra_dirs` entry that is an existing directory (no dedup, no sorting).
5. Empty -> 404.

On a typical device this includes `/opt/smhub-services/config` (from the `smhub-services` schema, i.e. `backend.db` and `stunnel.conf.d`), `/opt/zigbee2mqtt/data`, etc.

`metadata.json` (`:135-147`), written with `json.dump(indent=2)` to a temp file `/tmp/tmpXXXXXXXX.json`:
```json
{
  "device_id": "<EEPROM uuid as 32 hex chars, or null>",
  "timestamp": "<datetime.now().isoformat(): local time, no offset, microseconds>",
  "data_dirs": ["/home/smlight", "/opt/smhub-services/config", "..."],
  "apps": ["<every package name in /var/lib/opkg/status that is installed>"]
}
```

Archive: `engine.dispose()`, then inside `service_context(apps)` where `apps` = all installed package names except `smhub-services` (package names are used directly as OpenRC service names; only those reported `started` are stopped, then 2 s sleep, then restarted in reverse order after the archive is done) (`:151-221`), run:
```sh
tar -I "xz -1" -cf - --transform "s|tmp/tmpXXXXXXXX.json|metadata.json|" /tmp/tmpXXXXXXXX.json <dir1> <dir2> ...
```
(GNU tar; members have no leading `/`: `metadata.json`, `home/smlight/...`, `opt/smhub-services/config/backend.db`, ...). Output is streamed in 256 KiB chunks as `application/x-xz` with `Content-Disposition: attachment; filename=user_backup_YYYYMMDD_HHMMSS.tar.xz` (local time). Non-zero tar exit -> error after streaming. The temp metadata file is deleted.

### 8.2 Restore (`POST /stream/restore-backup?dry_run=false|true`, multipart `file`)

1. Save the upload in 64 KiB chunks to a temp file `*.tar.xz` in `/mnt/user` (fallback system temp); always deleted at the end (`:340-378`).
2. Validate (`:235-288`): open as `r:xz`; `metadata.json` member must exist; JSON with `data_dirs` list; `getmembers()` must succeed. Any failure -> 400 with a message.
3. `reset_dirs = data_dirs` without `/home/smlight`. `dry_run` -> `{"detail":"Backup validation successful (dry-run)","restored_dirs": reset_dirs}`.
4. `engine.dispose()`; inside `service_context(<installed packages except smhub-services>)`:
   - for each `reset_dirs` directory that exists: delete its contents (files, symlinks, sub-trees), keep the directory itself. **This includes `/opt/smhub-services/config/backend.db` while the backend runs.**
   - extract **all** members except `metadata.json` to `/` with `filter="fully_trusted"` (owners, modes, absolute or `..` paths all honoured).
5. `{"detail":"Restore completed successfully","restored_dirs": reset_dirs}`. No migration is run on the restored DB; the backend is not restarted.

`/home/smlight` content is merged, not cleared.

### 8.3 Factory backup

Created by the OS (not by the backend) in `/mnt/user/.factory-backup/backup-*.tar.xz`. The "current" file = the last of the matching names sorted in reverse lexical order (`:381-395`).
- `GET /stream/factory-backup` -> `{"available": true, "filename", "size", "metadata": <metadata.json if readable>}` or `{"available": false}`.
- `GET /stream/factory-backup/download` -> the file (`application/x-xz`).
- `POST /stream/factory-backup/restore?dry_run=` -> same restore as 8.2.
- `DELETE /stream/factory-backup` -> delete every `backup-*.tar.xz` there -> `{"success": true, "deleted": <bool>}` (`{"success": true}` if the dir is missing).

---

## 9. Files (`BE/api/routes/files.py`, `BE/core/file_provider.py`, prefix `/api/v1/files`, auth required)

### 9.1 Mounts (`BE/api/routes/files.py:28-79`, cached for the life of the process)

| Mount name | Physical path |
|---|---|
| `home` | home of user `smlight` (`os.path.expanduser("~smlight")`, normally `/home/smlight`) |
| `sdcard` | `/mnt/sdcard`, only if it exists at the first call |
| `<pkg>` | `system.data_dir.default` of every package **in the feed index** whose schema file exists (i.e. installed ones), absolute paths only; for a list: first element `<pkg>`, then `<pkg>_2`, `<pkg>_3`, ... |

### 9.2 Virtual paths and the `fileitem` table

- Each item has a virtual `path` `/<mount>/<relative path>`; mounts are rows with `parent_id = NULL`, `path = "/<mount>"`, `size = 0`.
- Physical path = `realpath(mount) / relative` resolved; must start with the mount's resolved path (`BE/core/file_provider.py:30-55`).
- `owner` = the Unix user name of the physical file's uid at sync time (`pwd.getpwuid`), or the numeric uid, or `unknown` (`:448-458`, `:521-531`).
- **Read-only rule**: every write (create folder, upload, save, copy (source and destination), move (source and destination), rename, delete) requires `item.owner == "smlight"` (the `DEVICE_USER`, `BE/core/config.py:61`), else `403 "Forbidden: You do not have permission to modify this file"` (`:15-22`).

Sync (`POST /files/sync[?parent_id=<id>]`, `:419-575`):
- Without `parent_id`: delete DB subtrees of mounts that are no longer configured (DB only); create missing mount dirs (`mkdir -p`) and rows; update mount owner; then recursive `_sync_directory`.
- `_sync_directory(dir, item)`: skip dirs named `.git`, `node_modules`, `__pycache__`, `.venv`, `.next`, `.cache`; DB children missing on disk are deleted (recursively, also trying to delete on disk); new disk entries get rows (`mime_type` from `mimetypes.guess_type`, files only; `size` = st_size for files, 0 for dirs); existing rows get size/owner updated; broken symlinks are skipped; recurse into dirs; set `synced_at = utcnow` on the directory row; flush every 500 changes.
- With `parent_id`: `_sync_directory` for that folder only.

### 9.3 Operations

| Endpoint | Effect | Ownership on disk |
|---|---|---|
| `GET /files/?parent_id=<id>` (or `root`) | list children (`root` = mounts, no param = everything) | |
| `GET /files/open?id=` | read file; UTF-8 -> `{id,name,content,isBinary:false}`, else base64 with `isBinary:true` | |
| `PUT /files/save {id, content}` | write UTF-8 text in place; update `size` | unchanged (existing file) |
| `POST /files/folder {name, parentId}` | `parentId` required (else 400 "Cannot create folders at the root mount level"; a mount row is a valid parent); 400 if the name exists; `mkdir -p`; row `owner='smlight'` | **root** (no chown) |
| `POST /files/upload` (multipart `file`, form `parentId`) | 400 if name exists; write; `shutil.chown(path, user="smlight")` (group unchanged); row owner `smlight`, `mime_type` = upload content type | owner `smlight`, group root |
| `POST /files/copy {sourceIds, destinationId}` | recursive copy (`shutil.copy2`), new rows owner `smlight`; 400 if the name exists in the destination | **root** |
| `PUT /files/move {sourceIds, destinationId}` | `shutil.move`, update paths of descendants | unchanged |
| `PATCH /files/rename {id, newName}` | `rename`, update descendant paths | unchanged |
| `DELETE /files/ {ids}` (or `paths`, treated as ids) | recursive delete on disk (`rmtree`/`unlink`) and in DB | |
| `GET /files/download?files=<id>&files=<id>` | one file -> the file; one dir -> `<name>.zip`; several -> `download.zip` (zip of files, paths relative to the parent dir), temp zip files are not deleted | |

API field names (aliases) for reference: `_id, name, isDirectory, path, parentId, size, mimeType, owner, createdAt, updatedAt, syncedAt` (`BE/models.py:372-385`).

---

## 10. Terminal (`BE/terminal.py`, `BE/api/routes/websocket.py:105-224`)

WebSocket `GET /ws/console` - **no authentication** (route has no dependency).

Spawn (`BE/terminal.py:23-75`): `pty.fork()`; in the child, for user `smlight`: `setgid(pw_gid)`, `initgroups("smlight", pw_gid)`, `setuid(pw_uid)`, env `HOME=<pw_dir>`, `USER=smlight`, `LOGNAME=smlight`, `SHELL=/bin/bash`, `cwd=<pw_dir>` (or `/tmp` if that fails), then `TERM=xterm-256color` and `execvp("/bin/bash", ["/bin/bash", "-l"])`. All other env vars of the backend are inherited (e.g. `PATH=/opt/bin:...`, `SMHUB_SERVICES_DATA`, `PKG_PATH`). User not found or privilege drop error -> child exits 1. Parent sets the master fd non-blocking.

Protocol:
- After spawn the text `PTY process started with PID <pid>` is broadcast to all **shared** WebSocket clients (not to the console socket) (`websocket.py:114`).
- Output: PTY bytes sent as binary frames (read with `select` 0.1 s, 1024-byte reads); EOF ends the session.
- Input: binary frames written to the PTY. Text frames: if they parse as a JSON object with `"type":"resize"`, `TIOCSWINSZ` with `rows` (default 24) and `cols` (default 80); other JSON objects are ignored; non-JSON text (or JSON that is not an object) is written to the PTY as UTF-8.
- Close: master fd closed, `SIGTERM`, up to ~100 ms wait, then `SIGKILL` (`BE/terminal.py:119-165`).

---

## 11. Logs

| Endpoint | File | Behaviour |
|---|---|---|
| `GET /api/v1/smhub/packages/{pkg}/log-history?filter=<word>` (auth) | `/var/log/messages` if `pkg == "messages"`, else `/var/log/<pkg>.log` (**no** name mapping); `pkg` must be in the feed index | whole file, non-empty lines stripped, case-insensitive substring filter; 404 if missing (`BE/api/routes/packages.py:416-452`) |
| WebSocket `/ws/logs/{app_id}?filter=<word>` (no auth) | `app_id` mapped through `LOG_FILE_MAPPING`, then `/var/log/messages` for `messages`, else `/var/log/<mapped>.log` | send the last 100 lines (filtered), then follow from the end, polling every 1 s; after 30 s without lines send a ping; file not found -> one `log_error` message (`BE/api/routes/websocket.py:42-102`, `BE/utils.py:49-85`) |

`LOG_FILE_MAPPING` (`SC/data.py:26-33`): `zwavejsui` -> `zwave-js-ui`, `smhub-ui` -> `nginx/error`, `openthread` -> `otbr-agent`, `nodered` -> `node-red`, `picoclaw-launcher` -> `picoclaw`, `tailscale` -> `tailscaled`.

Messages: `{"type":"log","level":"info","timestamp":"<local ISO>","message":"<line stripped>"}`, `{"type":"ping","timestamp":"..."}`, `{"type":"log_error","message":"Log file not found: <path>"}`. The follow loop keeps the file descriptor open (no reopen after rotation).

Log files written by the backend itself: `/var/log/opkg.log` (opkg output, 3.3), `/var/log/universal_silabs_flasher.log` (6.3); stdout/stderr of the process go to `/var/log/smhub-services.log` via supervise-daemon (`PKG/CONTROL/openrc:11-12`).

---

## 12. Modem (`BE/modem_controller.py`, routes `BE/api/routes/modem.py`)

### 12.1 Start conditions

Started in the background after startup (`BE/main.py:145-191`, `:348-352`):
1. GPIO provider created (section 13). No provider -> skip.
2. Pin `4g_check` must exist (it does not on `nano*` models, which have no pins); configure it as input; `get_value` must be **true** (active-low line, so true = physical low = module present).
3. `start_modem_controller()` (`BE/modem_controller.py:797-869`): read the EEPROM again; only models exactly `essential` or `premium`; create the controller and `connect()`; on failure the controller is discarded.
4. LED: if page `lte` setting `led_control` lowercased is `true`, configure `4g_led` and set it to **1** (bug list).
5. Start the monitor loop.

All `/api/v1/modem/*` routes return `501` unless the EEPROM model is `essential` or `premium` (`BE/api/routes/modem.py:18-31`); no other auth.

### 12.2 Port detection (`:122-204`)

1. `lsusb` output must contain `1e0e:9011` (SIMCom A76XX).
2. For each `/sys/bus/usb-serial/devices/ttyUSB*`: realpath; walk its path components, at the first component containing `:` take `<prefix>/../idVendor` and `idProduct`; if they equal `1e0e`/`9011` and `/dev/<ttyUSBn>` exists, keep it.
3. Sort; the command port is the **last** one (e.g. `/dev/ttyUSB2`).

### 12.3 Serial protocol

- `serial_asyncio.open_serial_connection(url=<port>, baudrate=115200, timeout=5.0)`.
- On connect: send `ATE0` (wait up to 1 s), sleep 0.2 s, send `AT` (wait up to 3 s). Success either way once the port opened (`:237-285`).
- Commands are sent as `<cmd>\r\n` in ASCII (`:308-363`). A response is the **first** non-unsolicited line read after sending (one line only, often `OK`).
- Reader: splits on `\n`, strips lines (`:365-388`). Unsolicited lines (prefixes `+CEREG:`, `+CREG:`, `+CGREG:`, `+CNSMOD:`, `+CGEV:`, `+CSQ:`, `+CPIN:`, `+CMTI:`, `+CDSI:`, `+CBM:`, `+CDS:`, `+CTZV:`, `RING`, `NO CARRIER`, `BUSY`, `NO ANSWER`, `CONNECT`, `RDY`, `+CFUN:` and the extra list at `:427-453`) are never given to commands; those starting with `+CSQ:`, `RDY`, `+CFUN:`, `+CEREG:` are parsed and sent to the status callback.
- Status callback: broadcast to shared WS clients (only if there are any) `{"type":"modem_status","timestamp":"<local ISO>","data":<parsed>}` (`:15-39`).

Parsing (`:457-551`):
- `+CSQ: <rssi>,<ber>` -> `{"type":"signal_quality","rssi":int,"rssi_dbm": null if rssi==99 else -113+2*rssi,"ber": null if 99}`.
- `RDY` -> `{"type":"modem_ready"}`; `+CFUN:...` -> `{"type":"functionality","status":"<line>"}`.
- `+CEREG:` values split on `,`: if there are >= 2 values, the first is 0..2 and the second 0..11 -> solicited `<n>,<stat>[,<tac>,<ci>]`; else unsolicited `<stat>[,<tac>,<ci>]`. Result `{"type":"lte_registration","status":stat,"status_text":...,"connected": stat in (1,5)}` plus `n`, `tac`, `ci` when present. Status texts: 0 not_searching, 1 registered_home, 2 searching, 3 denied, 4 unknown, 5 registered_roaming, 6 registered_sms_only, 11 emergency_only, else `invalid_status_<n>`. Parse errors -> `{"type":"lte_registration","raw","parse_error"}`.
- `stat == 3` (denied) triggers PDP context clearing (`:585-625`): `AT+CGDCONT?` (5 s) -> context ids from `+CGDCONT: <id>,...` lines (fallback `[1,2,3]`); then `AT+CGACT=0` and `AT+CGDCONT=<id>` for each, without waiting for answers.

Periodic and on-demand queries (sent without waiting for an answer, results come back as unsolicited lines):
- Monitor loop (`:771-794`): connected -> `AT+CSQ`, then sleep **120 s**; disconnected -> `connect()` retry, sleep 10 s.
- New shared WebSocket client (`/ws/{uuid}`): `AT+CSQ` and `AT+CEREG?` (`BE/api/routes/websocket.py:238-247`).
- `POST /modem/signal` -> `AT+CSQ`; `POST /modem/network` -> `AT+CEREG?` (400 if not connected).
- `POST /modem/connect` (initialize if needed, then open the port), `POST /modem/disconnect` (stop reader, close port; the monitor loop will reconnect after 10 s), `POST /modem/initialize`, `GET /modem/status` -> `{connected, port, baudrate, last_update}`.
- Defined but never called: `get_modem_info` (`AT+CGMI`, `AT+CGMM`, `AT+CGMR`, `AT+CGSN`), `setup_modem_config` (`AT+DIALMODE?`/`AT+DIALMODE=0`, `AT$MYCONFIG?`/`AT$MYCONFIG="usbnetmode",1`) (`:653-750`).

### 12.4 Power / reset / LED

The backend never powers the modem by itself. The UI runs `POST /gpio/4g_power/sequence {"value":"4g_power_on"}` (`FE/pages/Settings/Lte.jsx:80-83`). At boot the OS `modem-power` service runs `gpioset -t 100ms,100ms,0 4g_power=1` (stop: `-t 100ms,2500ms,0 4g_power=0`) when `lte.autostart` exists (`OS/etc/init.d/modem-power:38-46`). Sequences: section 13. The `4g_power` entry in `SC/data.py:100-105` (`gpioset ... -c gpiocgip4 1=1`, misspelt chip) is unused.

---

## 13. GPIO (`SC/gpio_provider.py`, routes `BE/api/routes/gpio.py`, no auth)

### 13.1 Chip mapping (`SC/gpio_provider.py:22-80`)

Logical port -> `/dev/gpiochipN`, chosen by `/etc/os-release` `VERSION_ID` parsed as in 7.1:

| Port | OS < 0.8.0 | OS >= 0.8.0 (or unparsable -> old) |
|---|---|---|
| porta | gpiochip0 | gpiochip1 |
| portb | gpiochip1 | gpiochip2 |
| portc | gpiochip2 | gpiochip3 |
| portd | gpiochip3 | gpiochip4 |
| porte | gpiochip4 | gpiochip0 |

(Missing `/etc/os-release` -> old mapping.)

### 13.2 Pins (`SC/gpio_provider.py:125-184`)

If the EEPROM model contains `nano` (case-insensitive) the pin and sequence lists are **empty**. Otherwise (`essential`, `premium`):

| Label | Port | Offset | Direction | Active low | Bias | Notes |
|---|---|---|---|---|---|---|
| `4g_led` | porte | 4 | output | **no** | - | |
| `4g_power` | porte | 1 | output | yes (default) | - | |
| `4g_reset` | porte | 24 | output | yes | - | |
| `4g_check` | portc | 11 | input | yes | - | true = module present |
| `zwave_check` | portc | 8 | input | yes | - | |
| `ups_check` | portc | 18 | input | yes | pull-up | "ups_pwr" on schematics |
| `ups_reset` | portc | 19 | output | yes | - | |
| `poe_check` | porte | 0 | input | yes | - | |

Outputs are requested with initial value inactive (`initial_value=0`), consumer name `system-config` (`SC/gpio_provider.py:20`, `:258-298`). Every route call re-requests the line (`configure_pin` releases an existing request first).

### 13.3 Sequences (`SC/gpio_provider.py:186-214`, `:356-379`)

Each step `(delay_ms, value)`: set the value (logical, i.e. after active-low inversion), **then** sleep `delay_ms`.

| Id | Pin | Steps | Physical effect (active-low pin) |
|---|---|---|---|
| `4g_power_on` | `4g_power` | (0, 1), (100, 0), (100, 1) | low, high for 100 ms, low (100 ms) |
| `4g_power_off` | `4g_power` | (0, 1), (2500, 0), (100, 1) | low, high for 2.5 s, low |
| `4g_reset` | `4g_reset` | (0, 1), (2500, 0), (100, 1) | low, high for 2.5 s, low |

### 13.4 Routes and reads

- `GET /gpio/{label}/read` -> configure the pin as listed, `get_value` -> `{"label","value": bool}` (logical value; for the `*_check` inputs, true = physical low). Missing label 404, provider missing 503, invalid chip 400.
- `POST /gpio/{label}/write {"value": bool}` -> `{"label","status":"ok"}`.
- `POST /gpio/{label}/sequence {"value": "<sequence id>"}` -> configures `<label>` then runs the sequence (the sequence's own pin is used).
- `GET /gpio/list` -> `{"pins":[{label, chip, pin, direction}]}`.
- The OS uses DT line names with the same labels (`gpioget --numeric 4g_check`, `gpioset 4g_power=1`, `OS/etc/init.d/modem-power:21`, `:40`); the backend uses chip+offset.

---

## 14. Device identity and system info

### 14.1 EEPROM (`SC/os_info.py:26`, `:172-264`)

Device `/sys/bus/i2c/devices/1-0050/eeprom`, first 46 bytes.
- Byte 0 == `0x01` -> V1 header, little-endian `struct "<B15s16s6sQ"`: `version` (u8), `model` (15 bytes, trailing `0x00`/`0xFF` stripped, ASCII), `uuid` (16 bytes -> 32 lowercase hex chars), `hw_rev` (6 bytes, stripped), `timestamp` (u64).
- Else legacy `"10s6s16s"`: model (10 bytes up to the first NUL), hw_rev (6 bytes up to NUL), uuid (16 bytes hex); `version=0`, `timestamp=None`.
- Read failure -> `None`. Mocks when the real device is absent: `/mock-eeprom`, `<repo>/rootfs/mock-eeprom`, `<repo>/eeprom/eeprom`, or env `SMHUB_MOCK_EEPROM_MODEL` (`BE/main.py:44-91`).
- Model strings in use: `essential`, `premium`, `nanop1`, `nanop7`, `nanop10`, `nanomg21`, `nanomg24`, `nanomg26`, `nanomr1`..`nanomr7` (6.1). Comparisons are mostly on the lowercased model, but `modem` uses exact `essential`/`premium`, migration `ac49055b2b34` uses case-sensitive `"mg" in model`.
- `GET /utils/os/eeprom` (no auth) -> `{model, hw_rev, uuid, version, timestamp}` or null.

### 14.2 OS release (`SC/os_info.py:63-79`)

`/etc/os-release`: every line with `=`; key lowercased, value with surrounding `"` stripped. Fields used: `name`, `id`, `version`, `version_id`, `pretty_name` (missing -> `""`). `GET /utils/os/version` (no auth) returns them. `version_id` drives the GPIO chip mapping.

### 14.3 Hostname and USB mode in app state

At lifespan start `app.state.hostname` and `app.state.usb_mode` are read through `ProviderController("hostname"/"usb_mode")` **before** the registry is filled, so they always fall back to `socket.gethostname()` and `"device"` (`BE/main.py:94-115`, `:341-342`). `GET /metrics/other` therefore always returns `{"usb_mode": "device"}` today (bug list); the UI uses it to decide whether to switch the USB mode before powering the modem.

### 14.4 Metrics (`BE/api/routes/performance.py`, `SC/performance.py`, no auth)

- `GET /metrics/performance` -> `{cpu:{percent}, memory:{total,available,percent,used,free}, load_average:{load1,load5,load15}, uptime}`; CPU percent from `psutil.cpu_percent(interval=None)` (since the previous call), uptime = now - boot time (s).
- `GET /metrics/data` -> `{disk: usage of /mnt/user (zeros if missing), sdcard: usage of /mnt/sdcard or null, network: {<iface>: {is_up, ipv4_address, ipv6_address, mac_address}}}` for interfaces starting with `eth`, `wlan`, `usb`, `wg` (last address of each family wins).
- `GET /metrics/other` -> `{"usb_mode": ...}` (14.3).

### 14.5 Services status (`BE/api/routes/utils.py:289-357`, auth)

- `GET /utils/os/services/status` -> `{<service>: <status>}` for `mosquitto`, `smhub-buzzer-daemon`, `smhub-ir-daemon`, `smhub-ambilight-daemon`, `smhub-mqtt-bridge`, `networkmanager`, `ser2net` (only if `/etc/ser2net/ser2net.yaml` exists), plus the `key` of every `apps` row with `is_service=1 AND installed=1` (the **package name**, not the resolved service name). Status = first word of the value in `rc-status -a -f ini` (`started`, `stopped`, `crashed`, ...); services not listed -> `inactive`.
- `POST /utils/os/services/{name}/{action}`: 404 unless `/etc/init.d/<name>` exists; `status` -> `{"status": <status or "stopped">}`; any other action string is passed to `rc-service <name> <action>` unchecked -> `{"status":"ok"}`.

### 14.6 Other OS commands (`BE/api/routes/utils.py`, no auth)

| Endpoint | Command |
|---|---|
| `GET /utils/os/reboot` | `reboot` |
| `GET /utils/os/shutdown` | `poweroff` |
| `GET /utils/os/list-usb` | `lsusb -v` -> `{"usb_devices": "<stdout>"}` |
| `GET /utils/os/list-usb-serial` | `ls -1 /dev/serial/by-id` -> list of names (empty on error) |
| `GET /utils/health-check/` | returns `true` |

Commands run without a shell, stdout/stderr captured; non-zero exit raises (`SC/provider.py:411-429`).

---

## 15. Tailscale and OpenThread

### 15.1 Tailscale (`BE/api/routes/tailscale.py`, prefix `/api/v1/smhub/tailscale`, auth + package installed)

Installed check: `Packages.is_installed("tailscale")` (3.1) else 404.

| Endpoint | Commands | Response |
|---|---|---|
| `GET /status` | `/opt/bin/tailscale status --active --json` | `{"data": <parsed JSON>}`; non-zero -> 502 |
| `POST /authenticate {"auth_key": "..."}` | `rc-service tailscale start` (errors logged), sleep 1 s, `/opt/bin/tailscale up --authkey <key>` (the key passes through `shlex.quote` and is then given as one argv element, so a key with shell-special characters would get literal quotes) | `{"status":"ok"}`; empty key 400; non-zero 502 |
| `POST /disconnect`, `POST /logout` | intended `tailscale down` / `tailscale logout`, but the command ids `tailscale-disconnect` / `tailscale-logout` do not exist in `SC/data.py`, so both return 500 today | |

The schema of the `tailscale` package (`APPS/tailscale-pkg/CONTROL/schema.json`) names these endpoints and the status fields `BackendState`, `Self.Online`, `Self.DNSName`, `Self.TailscaleIPs.0/1`.

### 15.2 OpenThread (`BE/api/routes/openthread.py`, prefix `/api/v1/smhub/openthread`, auth + package installed)

`GET /status` -> `GET <OTBR_REST_API_URL or http://localhost:8081>/node` with `Accept: application/json`, timeout 2 s; non-200, unreachable, invalid or non-object JSON -> 502; else `{"data": <node object>}` (fields the UI shows: `NetworkName`, `State`, `NumOfRouter`, `RlocAddress`, `ExtPanId`).

---

## 16. SSH keys (`BE/api/routes/ssh.py`, `BE/sshd.py`, prefix `/api/v1/smhub/ssh`, auth)

- File: `/home/smlight/.ssh/authorized_keys` (`BE/sshd.py:32`). sshd also reads `/mnt/misc/factory/authorized_keys` and `/mnt/sdcard/authorized_keys` (`OS/etc/ssh/sshd_config`, `AuthorizedKeysFile`); the backend only manages the home one.
- Parse (`BE/sshd.py:44-59`): strip; empty or `#` lines ignored; split on whitespace into at most 3 parts; needs >= 2 -> `{algorithm, key, comment, raw}`. No algorithm or base64 validation.
- `GET /keys` -> `{"status":"ok","data":[...]}`.
- `POST /keys {"key_raw": "..."}`: stripped; must be one line and parse; `os.makedirs(~/.ssh, exist_ok=True)` (created by root, default mode, **no chown/chmod**); appended as `<key>\n` if no identical stripped line exists (file created by root, umask mode). Returns the list.
- `DELETE /keys {"key_raw": "..."}`: remove every line whose stripped text equals the key; rewrite the file (`"w"`, ownership kept); 404 if missing. Returns the list.

---

## 17. Startup sequence (`BE/main.py:337-360`, `:194-291`)

In order, with file and command effects:

1. `app.state.eeprom = get_eeprom_data()` (14.1).
2. `app.state.os_info = OS_Info()` (reads `/etc/os-release`).
3. `app.state.hostname` / `usb_mode` via providers -> always fallbacks (14.3).
4. `smhub_startup()`:
   1. `Packages().load()` (lists + status).
   2. `ensure_system_user` (2.5) - may insert `user` + `usersettings`.
   3. EEPROM read again.
   4. For every package in the feed index: `check_and_create_apps` (3.4). If installed: `sync_app_autostart` (3.8, may update `apps.start_at_boot`); hot-patched schema (3.6; runs `ls -1 /dev/serial/by-id` for zigbee2mqtt/openthread); `sync_app_settings`; `sync_app_metadata` (1.6). Per-package errors are logged and skipped.
   5. `sync_system_schema` (built-in sources and settings, 1.6).
   6. `sync_pages_autostart`: for every page that has a `start_on_boot` setting (`lte`, `ser2net`), store `"true"`/`"false"` = `check_autostart(<page key>)` if different (`BE/core/db.py:295-316`).
   7. `sync_model_serial_port_corrections` (3.6; DB only in practice).
   8. iframe proxy `init_on_startup` (3.11): rewrites `/opt/smhub-services/config/stunnel.conf.d/app-*.conf`, deletes stale ones, `rc-service stunnel restart`.
   9. Clear and fill the config registry from `configsource`/`configsetting`.
   10. RTOS sync (5.4): overlays copied/removed in `/mnt/misc/overlays`, ELF copied/patched in `/opt/firmware/smhub-rtos.elf`, `/opt/firmware/bluetooth_proxy_mode` written, `rc-service smhub-broker restart` when needed.
   11. Start NetworkManager monitoring (4.15).
5. Start the Unix socket server `/run/smhub-backend.sock` (unlink old socket first).
6. Background task: create the GPIO provider (13), then try the modem (12.1).

On shutdown: stop NM monitoring, stop the modem controller (`BE/main.py:294-309`). Nothing else is written.

TCP readiness: the OS `api-ready` service treats any 200/401 from `/api/v1/login/user-session` as ready (section 0). With the Python app, uvicorn answers only after the lifespan startup above has finished (**Inferred** from uvicorn/Starlette lifespan semantics), so "ready" means all of steps 1-5 are done.

---

## 18. Package scripts and update behaviour

Package files (`PKG/CONTROL/`): `control`, `postinst`, `prerm`, `postrm`, `openrc` (init script), `service` (legacy BusyBox init script), `db` (seed DB), `schema.json`, `CHANGELOG.md`. opkg keeps extra control files as `/var/lib/opkg/info/smhub-services.<name>` (the scripts reference `smhub-services.openrc`, `.db`, `.service`; **Inferred** from those paths). Package payload is `/opt/smhub-services/venv/...` (Python venv) and the symlink `/opt/bin/smhub-services -> /opt/smhub-services/venv/bin/smhub-backend` (`PKG/smhub-services.mk:25-31`, `:128-129`).

`control` (`PKG/CONTROL/control`): `Package: smhub-services`, `Version: 1.1.0-1~dev4`, `Architecture: riscv64`, `Required-OS-Version: 1.0.0`, `Depends: nodejs (>= 22.15.1-2), python3-abi-3.14`, `Recommends: smhub-broker`. 2.0's package keeps `Package`, the version scheme (3.2), `Required-OS-Version: 1.0.0` and the same `Depends` and `Recommends` lines.

`schema.json` of smhub-services itself: `system.data_dir.default = /opt/smhub-services/config`, `iframe_port 0`, `service false` (`PKG/CONTROL/schema.json`). This puts `/opt/smhub-services/config` into user backups (8.1) and the file manager mounts (9.1). Keep it.

### 18.1 OpenRC init script (`PKG/CONTROL/openrc`, installed as `/etc/init.d/smhub-services`)

```sh
#!/sbin/openrc-run
description="SMHub backend services"
supervisor="supervise-daemon"
command="/opt/bin/smhub-services"
command_args=""
output_log="/var/log/smhub-services.log"
error_log="/var/log/smhub-services.log"
pidfile="/var/run/smhub-services.pid"
retry="TERM/30/KILL/5"
export PATH=/opt/bin:$PATH
export SMHUB_SERVICES_DATA=/opt/smhub-services
export PKG_PATH=/var/lib/opkg/info
depend() { need localmount; after firstboot-upgrade; provide smhub-services; }
start_pre() {
  [ -d "$SMHUB_SERVICES_DATA" ] || { eerror ...; return 1; }
  checkpath --directory --mode 755 "$SMHUB_SERVICES_DATA/config/stunnel.conf.d"
  checkpath --file --mode 0644 --owner smlight:smlight ${output_log}
}
```
(abridged; exact text in the file). Runlevel **boot** (`/etc/runlevels/boot/smhub-services`). Runs as root.

### 18.2 `prerm` (`PKG/CONTROL/prerm:1-18`)

If `/var/run/smhub-services.pid` exists: `touch /tmp/smhub-services-was-running`, then `rc-service smhub-services stop` (OpenRC) or `/etc/init.d/S98smhub-services stop`; errors ignored. Runs for upgrade and removal. When upgrading from the Python package to the Rust one, opkg runs the **old** prerm (and old postrm) and the **new** postinst (**Inferred** from opkg/dpkg semantics).

### 18.3 `postinst` (`PKG/CONTROL/postinst:1-110`), in order

1. `find /opt/smhub-services/venv/lib -name sniffio -type d -exec rm -rf {} +` (cleanup of an orphaned dependency).
2. Detect the venv Python `X.Y` from `venv/pyvenv.cfg` (`version =` or `version_info =`), default `3.14` (`:10-19`).
3. If `/opt/bin/smhub-services` is not a symlink: `ln -s /opt/smhub-services/venv/bin/smhub-backend /opt/bin/smhub-services` (`:22-24`).
4. OpenRC: `cp -f /var/lib/opkg/info/smhub-services.openrc /etc/init.d/smhub-services`; if `/etc/runlevels/boot/smhub-services` is not a symlink (or the init script is missing): `ln -sf /etc/init.d/smhub-services /etc/runlevels/boot/smhub-services` and `rm -f /etc/init.d/S98smhub-services`. BusyBox init: `ln -sf /var/lib/opkg/info/smhub-services.service /etc/init.d/S98smhub-services` if missing (`:26-40`).
5. If `/opt/smhub-services/config/backend.db` does not exist: `mkdir -p` the config dir, copy `/var/lib/opkg/info/smhub-services.db` there, `chown -R smlight:smlight /opt/smhub-services/config` (`:42-47`).
6. **OTA deferral** (`:49-63`): if `/usr/bin/python<X.Y>` is not executable (the package was installed by an OTA into the inactive OS slot whose Python is newer than the running one): `opkg flag unpacked smhub-services` (so `opkg configure` re-runs postinst after reboot; the OS `firstboot-upgrade` does that, `OS/etc/init.d/firstboot-upgrade:99-117`), `touch /tmp/smhub-reboot-required`, exit 0 (no migration, no restart).
7. Migrations (`:65-76`): `DB_VERSION=$(sqlite3 /opt/smhub-services/config/backend.db "SELECT version_num FROM alembic_version;")`; `A_VERSION` = output of `find_alembic_revisions.sh` run in `venv/lib/pythonX.Y/site-packages/smhub_backend/` (prints the head revision: the revision id that no file names as `down_revision`; with several heads the newest file; `BE/find_alembic_revisions.sh:1-71`). If different: `cd` there and run `venv/bin/alembic upgrade head` (uses `alembic.ini` in that directory, DB URL from settings, `BE/alembic/env.py:31-35`).
8. Non-OpenRC only: create `/var/lib/mosquitto/passwd` (mosquitto-owned, 0600) if the dir is missing; run `/usr/lib/smhub/set_default_zigbee_z2m.sh` (`:78-89`).
9. If `/tmp/smhub-services-was-running` exists: start the service (`rc-service smhub-services start` or S98 script), remove the flag (`:91-101`).
10. Legacy: if `/tmp/smhub-services-upgrade` exists, `/etc/init.d/S98smhub-services start` (`:103-108`).

### 18.4 `postrm` (`PKG/CONTROL/postrm:1-32`)

1. Same `sniffio` cleanup.
2. If `$2` is non-empty (upgrade): print and exit 0.
3. Real removal: remove the `/opt/bin/smhub-services` symlink; OpenRC: remove `/etc/init.d/smhub-services` and `/etc/runlevels/boot/smhub-services`; BusyBox: remove `/etc/init.d/S98smhub-services`. The DB and config dir are kept.

### 18.5 Legacy BusyBox script (`PKG/CONTROL/service`)

`start-stop-daemon --start --background --make-pidfile --pidfile /var/run/smhub-services.pid -o -O /tmp/smhub-services.log --exec /opt/bin/smhub-services`; stop with up to 3 s wait then `KILL`. Only for pre-OpenRC OS images.

### 18.6 What the Rust package must keep

- Same package name, init script path `/etc/init.d/smhub-services`, runlevel `boot`, pidfile, log path, `start_pre` checks, `/opt/bin/smhub-services` as the entry point (the postinst only creates the symlink when it is missing, so a new postinst must replace an existing Python symlink explicitly).
- The `was-running` flag protocol between prerm and postinst (the **old** prerm creates the flag during the Python -> Rust upgrade).
- Seed DB copy only when no DB exists; never replace an existing DB.
- Bring an older DB to head `4cc919e10054` (section 19). 2.0's daemon does it at start, which also covers a database restored from a backup (8.2).
- OTA deferral semantics (`opkg flag unpacked` + `/tmp/smhub-reboot-required`) if the new binary can fail in the pre-reboot window.
- The `/opt/smhub-services/config/stunnel.conf.d` dir and `/opt/smhub-services/config/backend.db` path.

---

## 19. Alembic revisions -> plain SQL

Revision chain (head last): `6e89cce31fda` -> `65450f9768ca` -> `ac49055b2b34` -> `e1ad05962702` -> `826b9fba7761` -> `b3f1c2d4e5a6` -> `0958a1a2873c` -> `71b1eaf89bfe` -> `7e4b48ebfc95` -> `f2a4d8c9b1e7` -> `9375c2db72a0` -> `4cc919e10054`.

Every step also runs `UPDATE alembic_version SET version_num = '<new>' WHERE version_num = '<old>';` (alembic). A fresh DB first gets `CREATE TABLE alembic_version (version_num VARCHAR(32) NOT NULL, CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num));` and `INSERT INTO alembic_version (version_num) VALUES ('6e89cce31fda');`. All steps of one `upgrade head` run in one transaction (**Inferred**: alembic default `transaction_per_migration=False`).

`6e89cce31fda` "Consolidated Initial Schema" (`BE/alembic/versions/00_consolidated_initial_schema.py`, `down_revision = None`). Uses the id of the last pre-consolidation migration, so databases that were already at `6e89cce31fda` skip it. Databases at older ids (`6c729a7276f1`, `be62b68127cf`, `a98c81d2bc6b`) cannot be upgraded by alembic (revision not found) - treat as unsupported.
```sql
CREATE TABLE user (id VARCHAR NOT NULL, email VARCHAR(255) NOT NULL, is_active BOOLEAN NOT NULL, is_superuser BOOLEAN NOT NULL, full_name VARCHAR(255), hashed_password VARCHAR NOT NULL, PRIMARY KEY (id));
CREATE UNIQUE INDEX ix_user_email ON user (email);
CREATE TABLE usersettings (id VARCHAR NOT NULL, light_theme BOOLEAN, metric_units BOOLEAN, notifications BOOLEAN, language VARCHAR(8), locale VARCHAR(8), timezone VARCHAR(64), use_12h_clock BOOLEAN, PRIMARY KEY (id), FOREIGN KEY(id) REFERENCES user (id));
CREATE TABLE apps (id INTEGER NOT NULL, "key" VARCHAR(32) NOT NULL, name VARCHAR(32) NOT NULL, version VARCHAR(16) NOT NULL, description VARCHAR(1024), enabled BOOLEAN NOT NULL, start_at_boot BOOLEAN NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME, PRIMARY KEY (id), UNIQUE ("key"));
CREATE TABLE appsettings (id INTEGER NOT NULL, "key" VARCHAR(255) NOT NULL, value VARCHAR(1024) NOT NULL, updated_at DATETIME, app_id INTEGER NOT NULL, PRIMARY KEY (id), FOREIGN KEY(app_id) REFERENCES apps (id), CONSTRAINT uix_app_id_key UNIQUE (app_id, "key"));
CREATE INDEX ix_appsettings_updated_at ON appsettings (updated_at);
```

`65450f9768ca` "Consolidated schema v1":
```sql
CREATE TABLE pages (id INTEGER NOT NULL, "key" VARCHAR(32) NOT NULL, name VARCHAR(32) NOT NULL, description VARCHAR(1024), created_at DATETIME NOT NULL, updated_at DATETIME, PRIMARY KEY (id), UNIQUE ("key"));
CREATE TABLE devicesettings (id INTEGER NOT NULL, "key" VARCHAR(255) NOT NULL, value VARCHAR(1024) NOT NULL, updated_at DATETIME, page_id INTEGER NOT NULL, PRIMARY KEY (id), FOREIGN KEY(page_id) REFERENCES pages (id), CONSTRAINT uix_dev_id_key UNIQUE (page_id, "key"));
CREATE INDEX ix_devicesettings_updated_at ON devicesettings (updated_at);
```

`ac49055b2b34` "update default values for zigbee2mqtt" (data only; skipped if there is no `apps` row `zigbee2mqtt`):
```sql
-- :app = (SELECT id FROM apps WHERE key = 'zigbee2mqtt')
UPDATE appsettings SET value = '/dev/ttyS1' WHERE app_id = :app AND key = 'serial_port' AND value = '/dev/ttyS4';
UPDATE appsettings SET value = '11' WHERE app_id = :app AND key = 'zigbee_channel' AND updated_at IS NULL;
UPDATE appsettings SET value = ''   WHERE app_id = :app AND key = 'network_key'   AND updated_at IS NULL;
UPDATE appsettings SET value = :adapter WHERE app_id = :app AND key = 'adapter' AND updated_at IS NULL;
-- :adapter = 'ember' if the raw EEPROM model string contains "mg" (case-sensitive), else 'zstack' (also when the EEPROM cannot be read)
```

`e1ad05962702` "Add watchdog column for apps":
```sql
ALTER TABLE apps ADD COLUMN watchdog BOOLEAN;
```

`826b9fba7761` "Add FileItem models":
```sql
CREATE TABLE fileitem (name VARCHAR(255) NOT NULL, is_directory BOOLEAN NOT NULL, path VARCHAR(1024) NOT NULL, parent_id VARCHAR, size INTEGER, mime_type VARCHAR(128), id VARCHAR NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME, PRIMARY KEY (id), FOREIGN KEY(parent_id) REFERENCES fileitem (id));
```

`b3f1c2d4e5a6` "Add installed, iframe_port, and icon columns to apps" (file header says `Revises: e1ad05962702`, the code says `down_revision = '826b9fba7761'`; the code is what counts):
```sql
ALTER TABLE apps ADD COLUMN installed BOOLEAN DEFAULT 0 NOT NULL;
ALTER TABLE apps ADD COLUMN iframe_port INTEGER;
ALTER TABLE apps ADD COLUMN icon VARCHAR(64);
```

`0958a1a2873c` "Add synced_at to FileItem" (file name says children_synced):
```sql
ALTER TABLE fileitem ADD COLUMN synced_at DATETIME;
ALTER TABLE fileitem ADD COLUMN owner VARCHAR(128) DEFAULT 'smlight' NOT NULL;
```

`71b1eaf89bfe` "Add is_service to Apps":
```sql
ALTER TABLE apps ADD COLUMN is_service BOOLEAN DEFAULT '0' NOT NULL;
```

`7e4b48ebfc95` "Add config source and setting models":
```sql
CREATE TABLE configsource (id INTEGER NOT NULL, name VARCHAR(64) NOT NULL, provider VARCHAR(32) NOT NULL, path VARCHAR(255) NOT NULL, params JSON, app_id INTEGER, PRIMARY KEY (id), FOREIGN KEY(app_id) REFERENCES apps (id));
CREATE UNIQUE INDEX ix_configsource_name ON configsource (name);
CREATE TABLE configsetting (id INTEGER NOT NULL, name VARCHAR(64) NOT NULL, source_name VARCHAR(64) NOT NULL, path VARCHAR(255) NOT NULL, value VARCHAR(1024), app_id INTEGER, PRIMARY KEY (id), FOREIGN KEY(app_id) REFERENCES apps (id), FOREIGN KEY(source_name) REFERENCES configsource (name) ON DELETE CASCADE);
```

`f2a4d8c9b1e7` "Add show_in_sidebar to apps":
```sql
ALTER TABLE apps ADD COLUMN show_in_sidebar BOOLEAN DEFAULT '1' NOT NULL;
```

`9375c2db72a0` "default update_at in settings to utcnow": no SQL (only the version row changes).

`4cc919e10054` "preseed_rtos_default_settings" (head):
```sql
-- if no page 'esphome':
INSERT INTO pages (key, name, description, created_at) VALUES ('esphome', 'ESPHome', 'Smart Home Hub RTOS settings', :utcnow);
-- :page = (SELECT id FROM pages WHERE key = 'esphome'); for each pair, only if (page_id, key) does not exist:
INSERT INTO devicesettings (key, value, updated_at, page_id) VALUES ('ambient_light', 'true', :utcnow, :page);
INSERT INTO devicesettings (key, value, updated_at, page_id) VALUES ('buzzer', 'true', :utcnow, :page);
INSERT INTO devicesettings (key, value, updated_at, page_id) VALUES ('user_i2c', 'true', :utcnow, :page);
INSERT INTO devicesettings (key, value, updated_at, page_id) VALUES ('bluetooth_proxy_mode', 'hci', :utcnow, :page);
INSERT INTO devicesettings (key, value, updated_at, page_id) VALUES ('encryption_key', '<factory-default key, base64, see BE/rtos.py:16>', :utcnow, :page);
-- :utcnow = naive UTC 'YYYY-MM-DD HH:MM:SS.ffffff'
```

2.0 reads `alembic_version` at start, applies the missing steps above in order inside one transaction and updates `alembic_version` as Alembic does, so 1.x can still be installed again (`crates/smhub-store/src/legacy.rs`). A database at a revision outside this chain is left as it is. A schema change of 2.0's own would need new revision ids, and 1.x's `alembic upgrade` would then fail on the unknown revision.

---

## 20. Users routes (`BE/api/routes/users.py`, prefix `/api/v1/users`)

| Endpoint | Who | Effect |
|---|---|---|
| `GET /users/?skip&limit` | superuser | list + count |
| `GET /users/me` | any user | own `UserPublic` |
| `PATCH /users/me {full_name?, email?}` | any user | 409 if the e-mail belongs to someone else; update |
| `PATCH /users/me/password {current_password, new_password}` | any user | 400 wrong current password; 400 if equal; store Argon2id hash |
| `DELETE /users/me` | non-superuser | 403 for superusers; delete user (ORM cascade deletes `usersettings`) |
| `POST /users/signup {email, password, full_name?}` | **any authenticated user** (including `system@localhost` via the socket) | 400 if the e-mail exists; create `is_active=1, is_superuser=0` + `usersettings` defaults |
| `GET /users/{id}` | self or superuser | 403 otherwise |
| `PATCH /users/{id} {email?, password?, is_active?, is_superuser?, full_name?}` | superuser | 404 missing; 409 e-mail taken; password re-hashed |
| `DELETE /users/{id}` | superuser | 404 missing; 403 deleting yourself; `DELETE FROM usersettings WHERE id=?` then delete user |
| `POST /users/create-mqtt-user {username, password}` | **nobody checked** | 2.6 |

`GET/POST /api/v1/smhub/user/settings` (auth): read / partial update of the caller's `usersettings` row (`light_theme, metric_units, notifications, language, locale, timezone, use_12h_clock`) (`BE/api/routes/data.py`).

E-mails are not validated (plain string, max 255). There is no e-mail sending, no password reset.

---

## 21. Authentication matrix of the old API (for reference)

| Router | Auth |
|---|---|
| `/login/*` | `cookie`, `access-token`, `security-status`: none; `user-session`, `test-token`: user |
| `/users/*` | per endpoint (section 20); `create-mqtt-user`: none |
| `/smhub/user/settings`, `/smhub/packages/*`, `/smhub/ssh/*`, `/smhub/tailscale/*`, `/smhub/openthread/*`, `/pages/*`, `/stream/*`, `/files/*` | user |
| `/utils/*` | none, except `/utils/os/services/*` (user) |
| `/net/*`, `/gpio/*`, `/radio/*`, `/metrics/*` | none |
| `/modem/*` | none (model check only) |
| `/dev/reset-db` | none, 404 outside `ENVIRONMENT=local` |
| WebSockets `/ws/logs/{app}`, `/ws/console`, `/ws/{uuid}` | none |

Shared WebSocket `/ws/{uuid}`: on connect sends the text `{"message": "Client #<uuid> connected"}`, triggers `AT+CSQ`/`AT+CEREG?`, then accepts `app_control` messages (3.8). Events broadcast to all shared clients: `nm_status`, `modem_status`, `rauc_progress`, `rauc_complete`, `rauc_error`, `radio_flash_status`, `radio_flash_progress`, and the PTY-started text.

---

## 22. Compatibility checklist (short form)

Things an existing device depends on, collected from the sections above:

- DB at `/opt/smhub-services/config/backend.db`, same tables/columns, value encodings of 1.2 readable (booleans `true/True/1`, lists JSON or Python repr), `updated_at IS NULL` = untouched default.
- Bring DBs at any revision from `6e89cce31fda` to `4cc919e10054` (19); keep `alembic_version`.
- `user-session` endpoint on `:8000/api/v1/login/user-session` answering 200/401 (OS `api-ready`).
- Argon2id verify + bcrypt verify with rehash. 1.x's JWT cookie `access_token` (HS256 `{sub, exp}`) does not carry over: 2.0 has its own sessions (D41).
- Unix socket `/run/smhub-backend.sock` acting as `system@localhost` (only if other local components use it; today only e2e tests are known).
- Flag files `/opt/share/services/<pkg>.autostart` + `rc-update add|del <svc> default`; `<pkg>.watchdog`; `lte.autostart`.
- Stunnel files `app-<key>.conf` with the same port allocation (users have bookmarked `https://<host>:<port>`), `rc-service stunnel restart`.
- Config files: mosquitto (flat, in-place activation of commented directives, multi-line `topic`), zigbee2mqtt YAML (dotted paths), zwave-js-ui JSON (indent 2), otbr-agent.conf (flat `KEY=value`), ser2net YAML v2 with `%YAML 1.1` header (in the form ser2net reads, D58), `/etc/hostname`, `/etc/timezone` + `/etc/localtime` link, `/etc/usb_mode`, `/etc/opkg/smlight.conf` dev line, `sshd_config` directives, `/mnt/user/.sdcard-config-enabled`.
- RTOS: `/opt/firmware/smhub-rtos.elf` patch from `/var/lib/opkg/info/esphome-bin.<nano|smhub>.elf`, overlays in `/mnt/misc/overlays`, `/opt/firmware/bluetooth_proxy_mode` (no newline), `rc-service smhub-broker restart`; run the sync at every start.
- NetworkManager connection names: Ethernet = the existing connection name (e.g. `Wired connection 1`) or `eth0`, Wi-Fi = SSID, WireGuard = `wg0` with interface `wg0`.
- Backups: same archive layout and `metadata.json`, so archives made by the old backend restore on the new one and vice versa; factory backup in `/mnt/user/.factory-backup/backup-*.tar.xz`.
- Mosquitto passwd entries in `$7$` PBKDF2-SHA512 format.
- Package scripts protocol of section 18.

---

## 23. Bugs and oddities

Behaviour that looks broken, dead or risky. 2.0 does not have to copy it; the facts that matter for compatibility are also in the main sections.

### Startup and registry

1. **Serial-port correction never reaches the config files.** `sync_model_serial_port_corrections` runs before the config registry is filled, so `ProviderController(app)` raises "Source not found" and only the DB is corrected (`BE/main.py:224-266`, `BE/core/db.py:399-410`).
2. **`app.state.hostname` / `usb_mode` are always fallbacks** (`socket.gethostname()`, `"device"`), because they are read before the registry exists (`BE/main.py:94-115`, `:341-342`). `GET /metrics/other` always says `device`.
3. **Registry is filled once per process.** After installing an app at runtime its `configsource` exists in the DB but not in memory, so `POST /smhub/packages/{pkg}/settings` fails with 500 until the backend restarts (`BE/main.py:237-266`, `BE/api/routes/packages.py:402-412`).
4. `check_and_create_apps` creates an `apps` row for **every** package of every feed (libraries included); `apps.version` is the feed version at creation time and is never updated; `apps.enabled` and `apps.icon` are never written (`BE/core/db.py:79-103`, `:106-161`).
5. `sync_app_settings` stores schema booleans as `0`/`1` (SQLite affinity), creates empty rows for `separator_*` keys, and gives toggle fields no `appsettings` row (`BE/core/db.py:189-230`).
6. `picoclaw-launcher`: `launcher_token` is linked to source `picoclaw-launcher` but the source is named `picoclaw`, so the setting is orphaned (FKs not enforced) and saving fails (`APPS/picoclaw-launcher/CONTROL/schema.json`).
7. stunnel is restarted at every backend start even if nothing changed; HTTPS ports are allocated (and reported) for apps that are not installed (`BE/core/iframe_proxy.py:131-154`).

### Value encodings

8. Booleans in `devicesettings`/`appsettings` are `True`/`False` from HTTP but `true`/`false` from startup sync and migrations; JSON `null` becomes the string `None`; app-setting lists become Python repr `['a']` (`BE/crud.py:205`, `:417`).
9. Seed `usersettings.locale` is `en-US`, the model default is `en_US`.

### Settings handlers

10. **Wi-Fi page never configures Wi-Fi with the 1.x UI**: the UI sends `security` = `WPA2`..., `ip_method` = `AUTO`/`STATIC`, the provider only accepts `wpa2-psk`... and `auto`/`manual`; validation fails silently, the handler still logs success and returns ok (`BE/page_handlers.py:331-342`, `SC/nm_providers.py:338-362`, `FE/schemas/wifi.json`). UI keys `hidden_network`, `ip_address`, `ipv4_gateway`, `ipv4_dns` are not read.
11. Ethernet: UI keys `dns` and `auto_connect_on_boot` are ignored (DNS from the UI never applied, autoconnect forced on); an invalid `ipv4.mtu` property is sent (`SC/nm_providers.py:868-880`, `SC/nm_controller.py:618-619`). The handler logs success even on failure (`BE/page_handlers.py:132-136`).
12. IPv4 DNS servers are sent as `int(IPv4Address)` in an `au` array; NM expects network byte order, so on little-endian RISC-V the address is reversed (**Inferred**) (`SC/nm_controller.py:670-674`).
13. `wpa2-psk+WPA3-psk` sends `key-mgmt "wpa-psk sae"` (not a valid NM value); WEP uses `wep-key-type 1` while the comment says passphrase (`SC/nm_providers.py:422-467`).
14. Every NM update uses `Update()` with a freshly built settings dict, wiping any property not built by the code (e.g. changes made with `nmcli` by the user).
15. `/net/wifi/down` deactivates a connection named `wifi_down` (never exists); `/net/wifi/active` looks up IP info for a connection named `wifi_active` (never exists); the Wi-Fi page "deactivate active Wi-Fi" step only deactivates a connection with the new SSID's name (`BE/api/routes/net.py:163-220`, `SC/nm_providers.py:596-634`).
16. WireGuard: no DNS support; validation errors are swallowed; the connection is not activated after saving.
17. LTE and ser2net handlers use `start_on_boot` default `False` when the key is missing (a partial save disables autostart); the updates handler uses channel default `stable` (a save without the channel removes the dev feed) (`BE/page_handlers.py:176`, `:266`, `:454`).
18. `rc-update add lte default` always fails (no `/etc/init.d/lte`; **Inferred**); only the flag file matters.
19. 4G LED: the page handler writes 0 for "LED off", the modem start writes 1 for the same setting (`BE/page_handlers.py:191`, `BE/modem_controller.py:853-855`).
20. Saving the `security` page (auth on/off) rewrites `sshd_config` (whitespace normalised) and restarts sshd although no SSH setting changed (`BE/page_handlers.py:463-473`, `BE/sshd.py:61-81`).
21. The `general` page time zone change does nothing if `/etc/timezone` is missing; `/utils/tz` changes only the symlink, so `/etc/timezone` and `/etc/localtime` can disagree (`SC/provider.py:476-483`, `SC/tz.py:49-61`).
22. ESPHome overlays use `bool(value)` on the raw HTTP value, so a string `"False"` enables the overlay (`BE/page_handlers.py:385-391`).
23. The flat editor's `get` returns values of commented-out lines too; `set` with fewer values than existing matches deletes commented example lines (`SC/flatconf.py:120-128`, `:221-224`).
24. YAML files are rewritten with alphabetically sorted keys and without comments (**Inferred** from `Cargo.lock`) (`RS/config.rs:27`).

### Packages

25. Status parsing treats `not-installed` as installed (substring match) (`SC/packages.py:365-367`).
26. Serial-port options from `ls -1 /dev/serial/by-id` are bare names, while the patterns require `/dev/serial/by-id/...`; `/utils/os/list-usb-serial` returns bare names too (`BE/core/db.py:461-472`).
27. Reset: list `data_dir`s are not deleted; schemas without `system.data_dir` skip the postinst; postinst runs without the `configure` argument; services keep running; app settings except `adapter` are only recreated at the next start (`SC/packages.py:498-528`).
28. The `adapter` default differs between code paths: reset uses `nanomg*` or `nanomr5` -> `ember`; migration `ac49055b2b34` uses case-sensitive `"mg" in model` (so `nanomr5` -> `zstack`); `set_default_zigbee_z2m.sh` uses case-insensitive `grep mg`.
29. `log-history` does not apply `LOG_FILE_MAPPING` (the WebSocket does), so e.g. zwavejsui history reads `/var/log/zwavejsui.log` instead of `zwave-js-ui.log` (`BE/api/routes/packages.py:434`).
30. `/utils/os/services/{name}/{action}` passes any action string to `rc-service` (`BE/api/routes/utils.py:333-357`); the services status list uses package names, not resolved service names (`smhub-ui` vs `nginx`) (`:308-316`).
31. Tailscale `disconnect`/`logout` reference command ids that do not exist -> always 500; `tailscale up` gets a `shlex.quote`d key as a single argv element (`BE/api/routes/tailscale.py:96-112`, `SC/provider.py:327-329`).
32. Unused command definitions: `mosquitto_passwd`, `4g_power` (misspelt chip `gpiocgip4`), two entries with id `add_group`, `rtos-notify`, `mosquitto_restart` only on non-OpenRC (`SC/data.py:63-136`).

### Security

33. No auth on: reboot, shutdown, RAUC install, time zone, hostname, all `/net`, `/gpio`, `/radio` (firmware flashing), `/modem`, `/metrics`, `create-mqtt-user`, and all WebSockets including `/ws/console` (an interactive shell as `smlight`) (section 21). The server listens on `0.0.0.0:8000`.
34. The JWT key is the same on every device (shipped `.env`); the auth cookie is not HttpOnly; the factory admin password is hard-coded in the UI and is the same bcrypt hash on every device; "enabling auth" creates a second user but does not change the factory admin password.
35. The Unix socket is created with default permissions and grants `system@localhost` (**Inferred** reachable by any local user who can open it).
36. Backup restore extracts with `filter="fully_trusted"` (absolute paths, `..`, any owner); `extra_dirs` accepts any directory (e.g. `/`); restore clears `/opt/smhub-services/config` (the live DB) and does not migrate the restored DB (`BE/api/routes/stream.py:307-332`).
37. Mosquitto passwd rewrite drops the original owner/mode and is not followed by a mosquitto reload (2.6).
38. Wi-Fi passphrase, WireGuard private/preshared keys, remote MQTT password are stored in plain text in `devicesettings`.

### Other

39. Backups stop services by package name; services whose OpenRC name differs (via `system.service.name` or `SERVICE_MAPPING`) are not stopped (`BE/api/routes/stream.py:157-162`).
40. Download zips are created in the temp dir and never deleted (`BE/api/routes/files.py:174-208`).
41. Files created by "new folder" and "copy" are root-owned on disk but recorded as owner `smlight` until the next sync, which then marks them read-only (`BE/core/file_provider.py:100-112`, `:273-299`).
42. RAUC: `rauc_complete.message` is always empty (the error text is in an unforwarded `error` field); if the `Completed` signal never comes, the install guard stays set until restart (`BE/api/routes/utils.py:147-165`, `SC/rauc_controller.py:107-113`).
43. Modem command responses = the first non-unsolicited line, so multi-line answers are cut; `get_modem_info` and `setup_modem_config` are dead code; the PDP clearing on `denied` sends commands without waiting (`BE/modem_controller.py:390-417`, `:585-625`).
44. GPIO pins and sequences are empty on all `nano*` models; the radio reset chips are hard-coded paths that do not follow the OS-version chip mapping (`SC/gpio_provider.py:125-127`, `BE/api/routes/radio.py:164-206`).
45. The PTY start message is broadcast to every shared WebSocket client instead of the console client (`BE/api/routes/websocket.py:114`).
46. Migration file `b3f1c2d4e5a6` says `Revises: e1ad05962702` in its header but `down_revision = '826b9fba7761'`; `9375c2db72a0` is empty; the "consolidated" first revision reuses an old id, so DBs older than it cannot be upgraded (19).
47. `BACKEND_CORS_ORIGINS` is parsed but the CORS list is hard-coded; HTTPS origins only for `smhub.local` (`BE/main.py:379-398`).
48. `DEV_BUILD` mode in system-config is half-broken (`AUTOSTART_PATH` undefined in that branch, `SC/packages.py:24-34`).
