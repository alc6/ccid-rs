# ccid

Pure-Rust host driver for USB CCID smart-card readers.

`ccid` talks to the reader directly over USB with [nusb](https://crates.io/crates/nusb).
It does not use PC/SC, pcscd, libccid or any C library, so a program that uses it
can ship as one static binary (for example `aarch64-unknown-linux-musl`).

> **Status: early development.** v0.1 is not released yet. Today the crate can list
> CCID interfaces, read their class descriptor, power cards on, read the ATR and
> UID, and exchange APDUs. Card events and the ACS vendor module are in progress.

Not affiliated with Advanced Card Systems Ltd.

## Scope

The core follows public standards only:

- USB CCID 1.1: class descriptor, bulk and interrupt messages.
- PC/SC Part 3: pseudo-APDUs and the contactless ATR.
- ISO/IEC 7816-4: APDUs.

Readers must work at the **short APDU** or **short and extended APDU** exchange
level. TPDU and character level readers are rejected with `Error::Unsupported`.

Vendor commands live in feature-gated modules (`acs`).

## Supported readers

| Reader | VID:PID | Interface | Exchange level | Status |
|---|---|---|---|---|
| ACS ACR1552U-MW (WalletMate II) | `072f:2401` | 0 (contactless) | short and extended APDU | in progress |

## Examples

```sh
cargo run --example probe                          # list CCID interfaces
cargo run --example probe -- read --taps 3 --aid ppse   # ATR, card type, UID, SELECT
cargo run --example descriptor                     # dump the CCID class descriptor of each interface
```

Hardware tests are ignored by default. Run them with a reader plugged in and a
card on it:

```sh
cargo test --test hardware -- --ignored --test-threads=1
```

## Platforms

- **Linux** (x86_64, arm64): target platform. Needs a udev rule for non-root
  access, and pcscd must not hold the reader. Details to come.
- **macOS**: see [macOS](#macos) below. Opening a reader needs one `sudo`
  command first.
- **Windows**: not a target for v0.1.

## macOS

`usbsmartcardreaderd` (part of CryptoTokenKit) claims every CCID interface as
soon as a reader is plugged in. Opening an interface then fails with
`Error::Busy`. Reading the descriptor still works, because it does not claim
the interface.

Measured on macOS 27.2 with SIP enabled:

- `sudo launchctl bootout system/com.apple.usbsmartcardreaderd` is refused
  (`150: Operation not permitted while System Integrity Protection is engaged`).
- `sudo killall usbsmartcardreaderd` (SIGTERM) does not stop the daemon.
- `sudo killall -9 usbsmartcardreaderd` works. launchd starts the daemon only
  when a CCID interface appears, so the reader stays free until it is replugged
  or the Mac restarts. Run the kill again after each replug.

```sh
sudo killall -9 usbsmartcardreaderd
cargo run --example probe -- read
```

Side effects: while the daemon is stopped, macOS has no smart-card support for
any CCID reader. CryptoTokenKit, PC/SC applications, smart-card login and
YubiKey PIV stop working until the daemon comes back. Nothing is changed
permanently.

## Dependencies

`nusb`, `thiserror` and `log`. nusb brings in pure-Rust OS bindings:
`io-kit-sys` and `core-foundation-sys` on macOS, `linux-raw-sys` (through
`rustix`) on Linux. No C code is compiled, and the crate has no direct `-sys`
dependency. The library has no `unsafe` code (`#![forbid(unsafe_code)]`).

Minimum supported Rust version: 1.85.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
