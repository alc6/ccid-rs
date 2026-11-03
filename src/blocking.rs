//! Blocking versions of [`Reader`](crate::Reader) and [`Card`](crate::Card).
//!
//! Each method runs the async version to completion on the current thread.
//! Do not call these from inside an async task.
//!
//! ```no_run
//! # fn main() -> Result<(), ccid::Error> {
//! let mut reader = ccid::blocking::Reader::open_by_id(0x072f, 0x2401, 0)?;
//! let mut card = reader.connect(0)?;
//! println!("ATR {} ({})", card.atr(), card.atr().card_type());
//! println!("UID {:02X?}", card.get_uid()?);
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use futures_lite::StreamExt;
use futures_lite::future::block_on;

use crate::{Atr, CcidDescriptor, IccStatus, ReaderInfo, Response, Result, SlotEvent};

/// Blocking version of [`crate::Reader`].
#[derive(Debug)]
pub struct Reader {
    inner: crate::Reader,
}

impl Reader {
    /// See [`crate::Reader::open`].
    pub fn open(info: &ReaderInfo) -> Result<Reader> {
        crate::Reader::open(info).map(Reader::from)
    }

    /// See [`crate::Reader::open_by_id`].
    pub fn open_by_id(vendor_id: u16, product_id: u16, interface: u8) -> Result<Reader> {
        crate::Reader::open_by_id(vendor_id, product_id, interface).map(Reader::from)
    }

    /// The async reader inside.
    pub fn into_async(self) -> crate::Reader {
        self.inner
    }

    /// See [`crate::Reader::info`].
    pub fn info(&self) -> &ReaderInfo {
        self.inner.info()
    }

    /// See [`crate::Reader::descriptor`].
    pub fn descriptor(&self) -> &CcidDescriptor {
        self.inner.descriptor()
    }

    /// See [`crate::Reader::timeout`].
    pub fn timeout(&self) -> Duration {
        self.inner.timeout()
    }

    /// See [`crate::Reader::set_timeout`].
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.inner.set_timeout(timeout);
    }

    /// See [`crate::Reader::slot_status`].
    pub fn slot_status(&mut self, slot: u8) -> Result<IccStatus> {
        block_on(self.inner.slot_status(slot))
    }

    /// See [`crate::Reader::power_on`].
    pub fn power_on(&mut self, slot: u8) -> Result<Atr> {
        block_on(self.inner.power_on(slot))
    }

    /// See [`crate::Reader::power_off`].
    pub fn power_off(&mut self, slot: u8) -> Result<()> {
        block_on(self.inner.power_off(slot))
    }

    /// See [`crate::Reader::transmit`].
    pub fn transmit(&mut self, slot: u8, apdu: &[u8]) -> Result<Response> {
        block_on(self.inner.transmit(slot, apdu))
    }

    /// See [`crate::Reader::escape`].
    pub fn escape(&mut self, slot: u8, command: &[u8]) -> Result<Vec<u8>> {
        block_on(self.inner.escape(slot, command))
    }

    /// See [`crate::Reader::events`].
    pub fn events(&mut self) -> Result<SlotEvents> {
        block_on(self.inner.events()).map(|inner| SlotEvents { inner })
    }

    /// See [`crate::Reader::connect`].
    pub fn connect(&mut self, slot: u8) -> Result<Card<'_>> {
        block_on(self.inner.connect(slot)).map(|inner| Card { inner })
    }
}

impl From<crate::Reader> for Reader {
    fn from(inner: crate::Reader) -> Reader {
        Reader { inner }
    }
}

/// Blocking version of [`crate::Card`].
#[derive(Debug)]
pub struct Card<'r> {
    inner: crate::Card<'r>,
}

impl Card<'_> {
    /// See [`crate::Card::atr`].
    pub fn atr(&self) -> &Atr {
        self.inner.atr()
    }

    /// See [`crate::Card::slot`].
    pub fn slot(&self) -> u8 {
        self.inner.slot()
    }

    /// See [`crate::Card::transmit`].
    pub fn transmit(&mut self, apdu: &[u8]) -> Result<Response> {
        block_on(self.inner.transmit(apdu))
    }

    /// See [`crate::Card::get_uid`].
    pub fn get_uid(&mut self) -> Result<Vec<u8>> {
        block_on(self.inner.get_uid())
    }

    /// See [`crate::Card::select_aid`].
    pub fn select_aid(&mut self, aid: &[u8]) -> Result<Response> {
        block_on(self.inner.select_aid(aid))
    }

    /// See [`crate::Card::disconnect`].
    pub fn disconnect(self) -> Result<()> {
        block_on(self.inner.disconnect())
    }
}

/// Blocking iterator over [`crate::SlotEvents`].
#[derive(Debug)]
pub struct SlotEvents {
    inner: crate::SlotEvents,
}

impl SlotEvents {
    /// The async stream inside.
    pub fn into_async(self) -> crate::SlotEvents {
        self.inner
    }
}

impl Iterator for SlotEvents {
    type Item = Result<SlotEvent>;

    /// Wait for the next event. Returns `None` after the reader is unplugged.
    fn next(&mut self) -> Option<Self::Item> {
        block_on(self.inner.next())
    }
}
