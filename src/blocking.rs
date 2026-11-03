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

/// Blocking version of [`crate::wait_for_reader`].
pub fn wait_for_reader(filter: impl FnMut(&ReaderInfo) -> bool) -> Result<ReaderInfo> {
    block_on(crate::wait_for_reader(filter))
}

/// Blocking version of [`ReaderInfo::wait_for_replug`].
pub fn wait_for_replug(info: &ReaderInfo) -> Result<ReaderInfo> {
    block_on(info.wait_for_replug())
}

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

#[cfg(feature = "acs")]
impl Reader {
    /// See [`crate::Reader::acs`]. Requires the `acs` feature.
    pub fn acs(&mut self) -> Acs<'_> {
        Acs {
            inner: self.inner.acs(),
        }
    }
}

/// Blocking version of [`crate::acs::Acs`]. Requires the `acs` feature.
#[cfg(feature = "acs")]
#[derive(Debug)]
pub struct Acs<'r> {
    inner: crate::acs::Acs<'r>,
}

#[cfg(feature = "acs")]
impl Acs<'_> {
    /// See [`crate::acs::Acs::firmware_version`].
    pub fn firmware_version(&mut self) -> Result<String> {
        block_on(self.inner.firmware_version())
    }

    /// See [`crate::acs::Acs::serial_number`].
    pub fn serial_number(&mut self) -> Result<String> {
        block_on(self.inner.serial_number())
    }

    /// See [`crate::acs::Acs::beep`].
    pub fn beep(&mut self, duration: Duration) -> Result<()> {
        block_on(self.inner.beep(duration))
    }

    /// See [`crate::acs::Acs::leds`].
    pub fn leds(&mut self) -> Result<crate::acs::Leds> {
        block_on(self.inner.leds())
    }

    /// See [`crate::acs::Acs::set_leds`].
    pub fn set_leds(&mut self, leds: crate::acs::Leds) -> Result<crate::acs::Leds> {
        block_on(self.inner.set_leds(leds))
    }

    /// See [`crate::acs::Acs::ui_behaviour`].
    pub fn ui_behaviour(&mut self) -> Result<crate::acs::UiBehaviour> {
        block_on(self.inner.ui_behaviour())
    }

    /// See [`crate::acs::Acs::set_ui_behaviour`].
    pub fn set_ui_behaviour(
        &mut self,
        ui: crate::acs::UiBehaviour,
    ) -> Result<crate::acs::UiBehaviour> {
        block_on(self.inner.set_ui_behaviour(ui))
    }

    /// See [`crate::acs::Acs::polling_option`].
    pub fn polling_option(&mut self) -> Result<crate::acs::PollingOption> {
        block_on(self.inner.polling_option())
    }

    /// See [`crate::acs::Acs::set_polling_option`].
    pub fn set_polling_option(
        &mut self,
        option: crate::acs::PollingOption,
    ) -> Result<crate::acs::PollingOption> {
        block_on(self.inner.set_polling_option(option))
    }

    /// See [`crate::acs::Acs::set_rf_mode`].
    pub fn set_rf_mode(&mut self, mode: crate::acs::RfMode) -> Result<()> {
        block_on(self.inner.set_rf_mode(mode))
    }

    /// See [`crate::acs::Acs::field_status`].
    pub fn field_status(&mut self) -> Result<crate::acs::FieldStatus> {
        block_on(self.inner.field_status())
    }
}
