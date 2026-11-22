#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright 2026 SMLIGHT. Licensed under GPL-3.0 (LICENSES/GPL-3.0.txt): it uses the GPL-3.0 flasher libraries.
"""Flash an SMHUB radio for smhubd (decision D6).

This helper keeps radio flashing on the vendor flashers the Python backend used
(universal-silabs-flasher, smlight-cc-flasher), with the same reset wiring
(old `api/routes/radio.py` and `radio_patch.py`). It runs only while a radio is
being flashed.

Output: one JSON object per line on stdout:
  {"step": "..."}       a stage started
  {"progress": 42.0}    percent of the image written
  {"error": "..."}      the flash failed (exit status 1)
"""

import argparse
import asyncio
import json
import logging
import sys
import time

LOG_FILE = "/var/log/universal_silabs_flasher.log"


def emit(**kw):
    print(json.dumps(kw), flush=True)


_last = [-1]


def progress(current, total):
    if not total:
        return
    pct = int(current * 100 / total)
    if pct != _last[0]:
        _last[0] = pct
        emit(progress=float(pct))


def setup_logging():
    try:
        handler = logging.FileHandler(LOG_FILE)
    except OSError:
        handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(logging.Formatter("%(asctime)s %(name)s %(levelname)s %(message)s"))
    root = logging.getLogger()
    root.addHandler(handler)
    root.setLevel(logging.INFO)
    logging.getLogger("universal_silabs_flasher").setLevel(logging.DEBUG)


def inject_reset_targets():
    """Add the SMHUB reset wirings to universal-silabs-flasher (old `_ensure_usf_initialized`)."""
    from universal_silabs_flasher.const import RESET_CONFIGS, GpioPattern, GpioResetConfig, ResetTarget

    def pattern(a, b):
        return [
            GpioPattern(pins={a: True, b: True}, delay_after=0.1),
            GpioPattern(pins={a: False, b: False}, delay_after=0.1),
            GpioPattern(pins={a: True, b: False}, delay_after=0.1),
            GpioPattern(pins={a: True, b: True}, delay_after=0.5),
        ]

    configs = {
        "SMHUB": GpioResetConfig(chip="/dev/gpiochip3", chip_type=None, pattern=pattern(7, 6)),
        "NANO": GpioResetConfig(chip="/dev/gpiochip2", chip_type=None, pattern=pattern(11, 12)),
        "NANOMR": GpioResetConfig(chip=["/dev/gpiochip1", "/dev/gpiochip3"], chip_type=None, pattern=pattern(0, 11)),
        "ADDON": GpioResetConfig(chip="/dev/gpiochip3", chip_type=None, pattern=pattern(1, 0)),
    }
    for name, config in configs.items():
        value = name.lower()
        member = object.__new__(ResetTarget)
        member._name_ = name
        member._value_ = value
        ResetTarget._value2member_map_[value] = member
        ResetTarget._member_map_[name] = member
        ResetTarget._member_names_.append(name)
        RESET_CONFIGS[member] = config
    inject_multi_chip()


def inject_multi_chip():
    """Let a reset pattern span two GPIO chips (old `radio_patch.inject_multi_gpio_support`)."""
    import gpiod
    import universal_silabs_flasher.gpio as usf_gpio

    original = usf_gpio._send_gpio_pattern

    def send(chip, pattern):
        if not isinstance(chip, list):
            original(chip, pattern)
            return
        pins = list(pattern[0].pins.keys())
        chip_map = {pins[i]: path for i, path in enumerate(chip) if i < len(pins)}
        by_chip = {}
        for pin, path in chip_map.items():
            by_chip.setdefault(path, []).append(pin)
        requests = []
        try:
            for path, chip_pins in by_chip.items():
                req = gpiod.request_lines(
                    path=path,
                    consumer="universal-silabs-flasher",
                    config={
                        pin: gpiod.LineSettings(
                            direction=gpiod.line.Direction.OUTPUT,
                            output_value=gpiod.line.Value(pattern[0].pins[pin]),
                        )
                        for pin in chip_pins
                    },
                )
                requests.append((req, chip_pins))
            time.sleep(pattern[0].delay_after)
            for step in pattern[1:]:
                for req, chip_pins in requests:
                    values = {pin: gpiod.line.Value(int(state)) for pin, state in step.pins.items() if pin in chip_pins}
                    if values:
                        req.set_values(values)
                time.sleep(step.delay_after)
        finally:
            for req, chip_pins in requests:
                req.reconfigure_lines({pin: gpiod.LineSettings(direction=gpiod.line.Direction.INPUT) for pin in chip_pins})
                req.release()

    usf_gpio._send_gpio_pattern = send


async def flash_silabs(port, data, target, ieee):
    from universal_silabs_flasher.const import ApplicationType, ResetTarget
    from universal_silabs_flasher.firmware import GBLImage
    from universal_silabs_flasher.flasher import Flasher

    inject_reset_targets()
    reset = ResetTarget._member_map_[target]
    image = GBLImage.from_bytes(data)
    flasher = Flasher(
        probe_methods=((ApplicationType.GECKO_BOOTLOADER, 115200),),
        device=port,
        bootloader_baudrate=115200,
        bootloader_reset=(reset,),
    )
    emit(step="entering_bootloader")
    await flasher.enter_bootloader()
    emit(step="flashing")
    await flasher.flash_firmware(firmware=image, progress_callback=progress, run_firmware=True)
    await asyncio.sleep(1)
    if ieee:
        emit(step="writing_ieee")
        try:
            import zigpy.types

            new_ieee = zigpy.types.EUI64.convert(ieee)
            writer = Flasher(probe_methods=((ApplicationType.EZSP, 115200),), device=port)
            await writer.write_emberznet_eui64(new_ieee=new_ieee, force=True)
        except Exception as e:  # the old backend only logged this
            logging.getLogger(__name__).error("Failed to write IEEE address %s: %s", ieee, e)
            emit(step="ieee_failed")


async def flash_ti(port, data, key, ieee):
    from smlight_cc_flasher.flasher import Flasher

    flasher = Flasher(device=port, m33="P10" in key)
    emit(step="entering_bootloader")
    await flasher.async_init(buffer=data, bootloader_mode="smhub", ieee_address=ieee)
    await flasher.connect()
    emit(step="flashing")
    await flasher.flash(progress_callback=progress)


async def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--chip", choices=["ti", "silabs", "zwave"])
    p.add_argument("--port")
    p.add_argument("--target", choices=["SMHUB", "NANO", "NANOMR", "ADDON"])
    p.add_argument("--file")
    p.add_argument("--key", default="")
    p.add_argument("--ieee")
    p.add_argument("--check", action="store_true", help="only check that the libraries load")
    a = p.parse_args()
    setup_logging()
    if a.check:
        import smlight_cc_flasher.flasher  # noqa: F401
        import universal_silabs_flasher.flasher  # noqa: F401
        import zigpy.types  # noqa: F401

        inject_reset_targets()
        emit(step="ok")
        return
    with open(a.file, "rb") as f:
        data = f.read()
    if a.chip == "ti":
        await flash_ti(a.port, data, a.key, a.ieee)
    else:
        await flash_silabs(a.port, data, "ADDON" if a.chip == "zwave" else a.target, None if a.chip == "zwave" else a.ieee)
    emit(step="done")


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except Exception as e:
        logging.getLogger(__name__).exception("flash failed")
        emit(error=f"{type(e).__name__}: {e}")
        sys.exit(1)
