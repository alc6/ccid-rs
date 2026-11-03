# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- List CCID interfaces on the USB bus (`list_readers`).
- Parse the CCID class descriptor (`CcidDescriptor`), including the exchange level.
- Open and claim a CCID interface (`Reader::open`, `Reader::open_by_id`).
- Examples: `probe` (list, open) and `descriptor`.
