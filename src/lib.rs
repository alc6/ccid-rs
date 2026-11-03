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
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), ccid::Error> {
//! for info in ccid::list_readers()? {
//!     let descriptor = info.descriptor()?;
//!     println!(
//!         "{:04x}:{:04x} interface {}: {:?}",
//!         info.vendor_id(),
//!         info.product_id(),
//!         info.interface_number(),
//!         descriptor.exchange_level(),
//!     );
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
pub use message::IccStatus;
pub use reader::{Reader, ReaderInfo, list_readers};
