//! Host driver for USB CCID smart-card readers, in pure Rust.
//!
//! `ccid` talks to a reader directly over USB with [`nusb`]. It does not use
//! PC/SC, pcscd, libccid or any C library, so a program that uses it can ship
//! as one static binary.
//!
//! The core follows public standards only:
//!
//! - USB CCID 1.1 (the class descriptor and the bulk/interrupt messages),
//! - PC/SC Part 3 (pseudo-APDUs and the contactless ATR),
//! - ISO/IEC 7816-4 (APDUs).
//!
//! Readers must work at the short APDU or the extended APDU exchange level.
//! TPDU and character level readers are rejected with [`Error::Unsupported`].
//!
//! # Overview
//!
//! - [`list_readers`] finds CCID interfaces; [`ReaderInfo::open`] claims one
//!   and returns a [`Reader`].
//! - [`Reader::events`] reports cards arriving and leaving.
//! - [`Reader::connect`] powers a card on and returns a [`Card`], with
//!   [`Card::transmit`], [`Card::get_uid`] and [`Card::select_aid`].
//! - [`wait_for_reader`] waits for an unplugged reader to come back.
//! - [`blocking`] has the same API without async.
//! - The `acs` feature adds vendor commands for ACS readers (`acs` module).
//!
//! The async API works with any executor: nusb and the timer run their own
//! threads, and the crate does not depend on tokio.
//!
//! # Example
//!
//! ```no_run
//! use ccid::SlotEventKind;
//! use futures_lite::StreamExt;
//!
//! # async fn run() -> Result<(), ccid::Error> {
//! let info = ccid::list_readers()?.into_iter().next().ok_or(ccid::Error::NotFound)?;
//! let mut reader = info.open()?;
//! let mut events = reader.events().await?;
//! while let Some(event) = events.next().await {
//!     if event?.kind == SlotEventKind::CardPresent {
//!         let mut card = reader.connect(0).await?;
//!         let uid = card.get_uid().await?;
//!         println!("ATR {} UID {uid:02X?}", card.atr());
//!     }
//! }
//! # Ok(())
//! # }
//! ```
#![deny(missing_docs)]
#![forbid(unsafe_code)]

#[cfg(feature = "acs")]
pub mod acs;
mod apdu;
mod atr;
pub mod blocking;
mod card;
mod descriptor;
mod error;
mod events;
mod hotplug;
mod message;
mod protocol;
mod reader;
mod transport;

pub use apdu::{Response, StatusWord};
pub use atr::{Atr, CardType};
pub use card::Card;
pub use descriptor::{CcidDescriptor, ExchangeLevel, Features, Protocols, Voltages};
pub use error::{Error, Result, SlotError};
pub use events::{SlotEvent, SlotEventKind, SlotEvents};
pub use hotplug::wait_for_reader;
pub use message::IccStatus;
pub use reader::{Reader, ReaderInfo, list_readers};

/// Compiles the code examples of the README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
