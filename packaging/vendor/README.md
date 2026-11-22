# Vendored files

## flasher-lib-py314-riscv64.tgz

The Python libraries the radio flash helper (`helpers/radio-flash/radio-flash.py`,
decision D6) needs, for the hub's Python 3.14 on riscv64. They are the exact
files the `smhub-services` 1.1.0-1 package shipped in its environment, so
flashing behaves as it did: `universal-silabs-flasher` 1.0.2,
`smlight-cc-flasher` 0.1.7, `gpiod` 2.4.2, `zigpy` 1.1.1, `bellows` 0.49.0 and
their dependencies (43 packages with their `dist-info` metadata; compiled
modules are riscv64 builds from the OS's Buildroot).

Made on a hub running 1.1.0-1 with

```
/opt/smhub-services/venv/bin/python -I pack-libs.py
```

which walks the dependency metadata from those three packages and archives
each package's top-level modules and its `dist-info`, without `__pycache__`
and without `RECORD` files. The build unpacks it to
`/opt/smhub-services/flasher/lib`; Python writes its bytecode cache there on the
first flash.

Licences are those of the packages (see each `dist-info`); among them
`universal-silabs-flasher` and `bellows` are GPL-3.0.

## License

This directory is licensed under the GNU General Public License version 3
([LICENSES/GPL-3.0.txt](../../LICENSES/GPL-3.0.txt)), not the repository's
noncommercial license, because it carries GPL-3.0 libraries; so is the helper
that uses them, `helpers/radio-flash/`. The daemon only starts that helper as a
separate program while a radio is flashed.
