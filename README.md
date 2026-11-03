# ccid

Pure-Rust host driver for USB CCID smart-card readers.

`ccid` talks to the reader directly over USB with [nusb](https://crates.io/crates/nusb).
It does not use PC/SC, pcscd, libccid or any C library, so a program that uses it
can ship as one static binary (for example `aarch64-unknown-linux-musl`).

Not affiliated with Advanced Card Systems Ltd.

## Features

- List CCID interfaces and read their class descriptor (exchange level,
  maximum message size, slots, voltages, protocols).
- Card arrival and removal events from the reader's interrupt endpoint. The
  reader detects cards itself; the host does not poll.
- Power cards on, read the ATR (with the PC/SC Part 3 card type: MIFARE
  Classic 1K/4K, Ultralight, Plus, FeliCa, ISO 14443-4, ...), read the UID,
  select applications and exchange APDUs, including extended APDUs split over
  several messages.
- Typed errors: reader unplugged, timeout, no card, card removed during the
  exchange, reader slot errors, protocol errors, unsupported reader.
- Wait for an unplugged reader to come back (`wait_for_reader`).
- Vendor commands for ACS readers behind the `acs` feature: buzzer, LEDs,
  RF field on/off, polling interval, firmware version, serial number.
- Async API that works with any executor (no tokio dependency), plus a
  blocking wrapper (`ccid::blocking`).

The core follows public standards only: USB CCID 1.1, PC/SC Part 3
(pseudo-APDUs and the contactless ATR) and ISO/IEC 7816-4. Readers must work at
the **short APDU** or **short and extended APDU** exchange level; TPDU and
character level readers are rejected with `Error::Unsupported`.

## Supported readers

| Reader | VID:PID | Interface | Exchange level | Tested |
|---|---|---|---|---|
| ACS ACR1552U-MW (WalletMate II), firmware 5.00.14 | `072f:2401` | 0 (contactless) | short and extended APDU | macOS: all features. Linux arm64: static build only |

Other CCID readers at an APDU exchange level should work with the core, but
have not been tested. `cargo run --example descriptor` shows a reader's
exchange level without claiming it.

## Quickstart

The crate is not on crates.io yet:

```toml
[dependencies]
ccid = { git = "https://github.com/alc6/ccid-rs" }
futures-lite = "2"  # or any crate with a StreamExt
```

Read every card that is tapped on the first reader:

```rust,no_run
use ccid::SlotEventKind;
use futures_lite::StreamExt;

async fn run() -> Result<(), ccid::Error> {
    let info = ccid::list_readers()?.into_iter().next().ok_or(ccid::Error::NotFound)?;
    let mut reader = info.open()?;
    let mut events = reader.events().await?;
    while let Some(event) = events.next().await {
        if event?.kind != SlotEventKind::CardPresent {
            continue;
        }
        let mut card = reader.connect(0).await?;
        println!("ATR {} ({})", card.atr(), card.atr().card_type());
        println!("UID {:02X?}", card.get_uid().await?);
        let ppse = card.select_aid(b"2PAY.SYS.DDF01").await?;
        println!("SELECT PPSE: SW {}", ppse.sw);
    }
    Ok(())
}
```

The same without async:

```rust,no_run
fn main() -> Result<(), ccid::Error> {
    let mut reader = ccid::blocking::Reader::open_by_id(0x072f, 0x2401, 0)?;
    for event in reader.events()? {
        if event?.kind == ccid::SlotEventKind::CardPresent {
            let mut card = reader.connect(0)?;
            println!("UID {:02X?}", card.get_uid()?);
        }
    }
    Ok(())
}
```

A `Reader` runs one command at a time (`&mut self`). The event stream does not
borrow it, so you can read events and talk to the card from different tasks;
wrap the reader in your runtime's mutex to share it. The futures are `Send`.

## Examples

```sh
cargo run --example probe                               # list CCID interfaces
cargo run --example descriptor                          # dump each CCID class descriptor
cargo run --example probe -- read --taps 3 --aid ppse   # ATR, card type, UID, SELECT
cargo run --example probe -- watch --aid ppse           # every arrival and removal, with timing
cargo run --example probe -- watch --reconnect          # keep going after an unplug
cargo run --example acs --features acs                  # ACS reader settings
```

## Linux

Linux (x86_64 and arm64) is the deployment target. CI builds the examples as
static binaries for `x86_64-unknown-linux-musl` and
`aarch64-unknown-linux-musl` and keeps them as build artifacts. To build on an
arm64 Linux machine:

```sh
rustup target add aarch64-unknown-linux-musl
cargo build --release --target aarch64-unknown-linux-musl
```

The kernel has no CCID driver, so nothing holds the reader by default. Two
things to set up:

- **Permissions.** A non-root process needs write access to the reader's
  `/dev/bus/usb` node. Install [`contrib/udev/70-ccid.rules`](contrib/udev/70-ccid.rules)
  and add the service user to the group it names (installation steps are in
  the file). Without it, `open` fails with `Error::PermissionDenied`.
- **pcscd.** If pcscd is installed, it claims readers as soon as a PC/SC
  client starts it, and `open` fails with `Error::Busy`. Check with
  `systemctl status pcscd.socket pcscd.service`, and disable it if nothing on
  the device needs PC/SC:
  `sudo systemctl disable --now pcscd.socket pcscd.service`.

`lsusb -t` shows `Driver=usbfs` on an interface that a user-space program
holds, and `Driver=[none]` on a free one.

Status: the Linux USB code path has not been run on hardware yet.

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

## Windows

Not a target for v0.1. Nothing in the crate is Windows-specific, but it is not
built or tested there, and Windows' own CCID driver would hold the reader.

## ACS vendor commands

The `acs` feature adds `Reader::acs()` for ACR1552U-family readers. Byte
layouts come from section 6 of the
[ACR1552U Series Reference Manual v1.09](https://www.acs.com.hk/en/download-manual/13474/REF-ACR1552U-Series-1.09.pdf).

- Firmware version and serial number.
- Buzzer: one beep of 10 ms to 2.55 s.
- LEDs (blue, green): read and set. On the WalletMate II both on looks light
  blue. Its firmware answers the RGB LED command of the ACR1552U-A with `FF`
  and ignores it, so the crate does not offer it.
- UI behaviour: what the reader does on its own, such as a beep when a card
  arrives.
- Polling option: polling on or off, RF off interval (250, 500, 1000 or
  2500 ms, or none), MIFARE type in the ATR.
- RF field: off, on with polling, on without polling. Use it to pause polling
  while a nearby payment terminal works.

The manual says the reader saves the polling option and the UI behaviour, so
set them once when you set a reader up. The RF field mode and the LEDs are not
saved settings.

```sh
cargo run --example acs --features acs                       # show settings
cargo run --example acs --features acs -- beep 100
cargo run --example acs --features acs -- rf-off-interval 1000
cargo run --example acs --features acs -- pause 5            # field off 5 s, then on
```

### ACR1552U card timing

Measured on the WalletMate II with the factory polling option (`0x8B`: RF off
interval 250 ms):

- From the arrival event, the UID is read in about 5 ms: about 4.5 ms for
  IccPowerOn (which returns the ATR), then about 0.3 ms for GET UID.
- The reader checks that the card is still there about 0.9 s after
  activation, then about every 0.5 s. A removal is reported at the next check,
  so a quick tap always shows "removed ~0.9 s after arrival", and later
  removals show 0.9 s + n × 0.5 s. A card left on the reader stays present.
- After a removal, the reader detects the next card at the earliest about
  0.5 s later.
- With the RF off interval set to 1000 ms, these become about 1.6 s for the
  first presence check and about 1.25 s before the next card is detected.
- After the field is switched back on (`RfMode::OnWithPolling`), a card
  already on the reader is reported about 150 ms later.

## Non-goals for v0.1

- TPDU and character exchange levels, and the T=0 / T=1 protocols.
- A PC/SC-compatible API.
- Contact-card features beyond ATR and APDUs.
- Apple VAS / ECP and Google Smart Tap (they need vendor agreements).
- The reader's HID keyboard mode and card emulation mode.
- MIFARE Classic authentication and block read/write (planned later, through
  PC/SC Part 3 pseudo-APDUs).
- Windows.

## Testing

```sh
cargo test                                               # unit tests, no hardware
cargo test --all-features --test hardware -- --ignored --test-threads=1
```

Unit tests cover message encoding, the descriptor and ATR parsers, and the
command sequencing (sequence numbers, time extensions, chaining, timeouts,
errors) against a fake transport. The hardware tests are ignored by default:
run them with a reader plugged in and a card on it. One of them asks you to
remove the card.

## Dependencies

`nusb`, `thiserror`, `log`, `futures-lite` and `futures-timer`. nusb brings
in pure-Rust OS bindings: `io-kit-sys` and `core-foundation-sys` on macOS,
`linux-raw-sys` (through `rustix`) on Linux. No C code is compiled, and the
crate has no direct `-sys` dependency. The library has no `unsafe` code
(`#![forbid(unsafe_code)]`).

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
