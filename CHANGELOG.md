# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-03

First release. Tested on macOS with an ACS WalletMate II (ACR1552U family).
Builds as a static binary for `aarch64-unknown-linux-musl` and
`x86_64-unknown-linux-musl`; not yet run on Linux hardware.

### Added

- List CCID interfaces on the USB bus (`list_readers`).
- Parse the CCID class descriptor (`CcidDescriptor`), including the exchange
  level. TPDU and character level readers are rejected as unsupported.
- Open and claim a CCID interface (`Reader::open`, `Reader::open_by_id`).
- CCID commands: IccPowerOn, IccPowerOff, GetSlotStatus, XfrBlock, Escape and
  Abort, with sequence checks, time extensions, multi-packet replies, extended
  APDU chaining and recovery after a timeout.
- Card helpers: `Reader::connect` → `Card` with `transmit`, `get_uid`
  (`FF CA 00 00 00`) and `select_aid`.
- ATR parsing with PC/SC Part 3 card types (`Atr`, `CardType`).
- Typed errors for unplugged reader, timeout, missing card, card removed and
  reader slot errors.
- Card events from the interrupt endpoint (`Reader::events` → `SlotEvents`
  stream of `SlotEvent`), starting with the cards already present.
- Wait for a reader to be plugged in again (`wait_for_reader`,
  `ReaderInfo::same_interface_as`).
- ACS vendor commands behind the `acs` feature (`Reader::acs`): firmware
  version, serial number, buzzer, LEDs, UI behaviour, polling option (RF off
  interval) and RF field mode.
- Blocking wrapper (`ccid::blocking`), including an iterator over events and
  the ACS commands.
- Examples: `probe` (list, open, read, watch), `descriptor` and `acs`.
- udev rule for non-root access on Linux (`contrib/udev/70-ccid.rules`).

[Unreleased]: https://github.com/alc6/ccid-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/alc6/ccid-rs/releases/tag/v0.1.0
