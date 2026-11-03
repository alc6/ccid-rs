# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- List CCID interfaces on the USB bus (`list_readers`).
- Parse the CCID class descriptor (`CcidDescriptor`), including the exchange level.
- Open and claim a CCID interface (`Reader::open`, `Reader::open_by_id`).
- CCID commands: IccPowerOn, IccPowerOff, GetSlotStatus, XfrBlock, Escape and
  Abort, with sequence checks, time extensions, multi-packet replies and
  extended APDU chaining.
- Card helpers: `Reader::connect` → `Card` with `transmit`, `get_uid`
  (`FF CA 00 00 00`) and `select_aid`.
- ATR parsing with PC/SC Part 3 card types (`Atr`, `CardType`).
- Typed errors for unplugged reader, timeout, missing card, card removed and
  reader slot errors.
- Card events from the interrupt endpoint (`Reader::events` → `SlotEvents`
  stream of `SlotEvent`), starting with the cards already present.
- ACS vendor commands behind the `acs` feature (`Reader::acs`): firmware
  version, serial number, buzzer, LEDs, UI behaviour, polling option (RF off
  interval) and RF field mode.
- Blocking wrapper (`ccid::blocking`), including an iterator over events and
  the ACS commands.
- Examples: `probe` (list, open, read, watch), `acs` and
  `descriptor`.
