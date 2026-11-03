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
