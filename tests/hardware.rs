//! Tests against a real reader. They are ignored by default.
//!
//! Plug a reader in, put a card on it, then run:
//!
//! ```text
//! cargo test --test hardware -- --ignored --test-threads=1
//! ```
//!
//! They use the first CCID interface found. The reader must not be held by
//! another driver (see the README for macOS and Linux).

use ccid::{ExchangeLevel, Reader};
use futures_lite::future::block_on;

fn open_first_reader() -> Reader {
    let info = ccid::list_readers()
        .expect("list readers")
        .into_iter()
        .next()
        .expect("no CCID reader plugged in");
    info.open().expect("open reader")
}

#[test]
#[ignore = "needs a reader"]
fn opens_an_apdu_level_reader() {
    let reader = open_first_reader();
    assert!(matches!(
        reader.descriptor().exchange_level(),
        Some(ExchangeLevel::ShortApdu | ExchangeLevel::ExtendedApdu)
    ));
}

#[test]
#[ignore = "needs a reader and a card"]
fn powers_on_and_reads_uid() {
    let mut reader = open_first_reader();
    block_on(async {
        let mut card = reader.connect(0).await.expect("put a card on the reader");
        assert!(!card.atr().as_bytes().is_empty());
        let uid = card.get_uid().await.expect("GET DATA UID");
        // ISO 14443 UIDs are 4, 7 or 10 bytes.
        assert!(matches!(uid.len(), 4 | 7 | 10), "UID length {}", uid.len());
        card.disconnect().await.expect("power off");
    });
}

#[test]
#[ignore = "needs a reader and a person to move a card"]
fn reports_card_events() {
    use ccid::{SlotEvent, SlotEventKind};
    use futures_lite::StreamExt;
    use std::time::Duration;

    let mut reader = open_first_reader();
    block_on(async {
        let mut events = reader.events().await.expect("start events");
        let mut next = async || -> SlotEvent {
            let event = futures_lite::future::or(events.next(), async {
                futures_timer::Delay::new(Duration::from_secs(30)).await;
                None
            });
            event.await.expect("no event within 30 s").expect("event")
        };
        eprintln!("put a card on the reader (or leave it there), then remove it");
        let first = next().await;
        assert_eq!(first.kind, SlotEventKind::CardPresent);
        let second = next().await;
        assert_eq!(second.kind, SlotEventKind::CardRemoved);
    });
}

#[cfg(feature = "acs")]
#[test]
#[ignore = "needs an ACR1552U-family reader"]
fn acs_commands_answer_and_settings_round_trip() {
    use ccid::acs::Leds;

    let mut reader = open_first_reader();
    block_on(async {
        let mut acs = reader.acs();
        assert!(!acs.firmware_version().await.unwrap().is_empty());
        assert!(!acs.serial_number().await.unwrap().is_empty());
        // LEDs are not saved: switch them, then put them back.
        let leds = acs.leds().await.unwrap();
        let both = Leds {
            blue: true,
            green: true,
        };
        assert_eq!(acs.set_leds(both).await.unwrap(), both);
        acs.set_leds(leds).await.unwrap();
        // Writing the current polling option back leaves it unchanged.
        let option = acs.polling_option().await.unwrap();
        assert_eq!(acs.set_polling_option(option).await.unwrap(), option);
    });
}
