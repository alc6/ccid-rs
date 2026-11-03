//! Reader discovery and opening.

use std::fmt;
use std::time::Duration;

use nusb::MaybeFuture;
use nusb::descriptors::TransferType;
use nusb::transfer::{Bulk, Direction, In, Interrupt, Out};

use crate::apdu::Response;
use crate::atr::Atr;
use crate::card::Card;
use crate::descriptor::{CCID_DESCRIPTOR_TYPE, CcidDescriptor, ExchangeLevel};
use crate::events::SlotEvents;
use crate::message::IccStatus;
use crate::protocol::Protocol;
use crate::transport::UsbTransport;
use crate::{Error, Result};

/// USB interface class of CCID readers.
pub(crate) const CCID_CLASS: u8 = 0x0B;

/// List the CCID interfaces on the USB bus.
///
/// A USB device can have several CCID interfaces, for example a contactless
/// slot and a SAM slot. Each one is a separate [`ReaderInfo`]. Interfaces of
/// the same device are sorted by interface number.
///
/// This only reads cached OS data: it does not open any device.
pub fn list_readers() -> Result<Vec<ReaderInfo>> {
    let mut readers = Vec::new();
    for device in nusb::list_devices().wait()? {
        let mut found: Vec<_> = device
            .interfaces()
            .filter(|i| i.class() == CCID_CLASS)
            .map(|i| ReaderInfo {
                device: device.clone(),
                interface: i.interface_number(),
                interface_name: i.interface_string().map(str::to_owned),
            })
            .collect();
        found.sort_by_key(|r| r.interface);
        readers.extend(found);
    }
    Ok(readers)
}

/// A CCID interface found on the USB bus.
#[derive(Debug, Clone)]
pub struct ReaderInfo {
    pub(crate) device: nusb::DeviceInfo,
    interface: u8,
    interface_name: Option<String>,
}

impl ReaderInfo {
    /// USB vendor ID.
    pub fn vendor_id(&self) -> u16 {
        self.device.vendor_id()
    }

    /// USB product ID.
    pub fn product_id(&self) -> u16 {
        self.device.product_id()
    }

    /// USB interface number of this CCID interface.
    pub fn interface_number(&self) -> u8 {
        self.interface
    }

    /// Interface string, if the OS cached it.
    pub fn interface_name(&self) -> Option<&str> {
        self.interface_name.as_deref()
    }

    /// Manufacturer string of the USB device, if the OS cached it.
    pub fn manufacturer(&self) -> Option<&str> {
        self.device.manufacturer_string()
    }

    /// Product string of the USB device, if the OS cached it.
    pub fn product(&self) -> Option<&str> {
        self.device.product_string()
    }

    /// Serial number string of the USB device, if the OS cached it.
    pub fn serial_number(&self) -> Option<&str> {
        self.device.serial_number()
    }

    /// Whether `other` is the same interface of the same reader model and,
    /// when the reader reports one, the same serial number. Use it to find
    /// a reader again after it was unplugged; see
    /// [`wait_for_replug`](Self::wait_for_replug).
    pub fn same_interface_as(&self, other: &ReaderInfo) -> bool {
        self.vendor_id() == other.vendor_id()
            && self.product_id() == other.product_id()
            && self.interface == other.interface
            && self.serial_number() == other.serial_number()
    }

    /// Read the CCID class descriptor of this interface.
    ///
    /// This opens the USB device but does not claim the interface, so it
    /// works while another driver holds the interface. On Linux it still
    /// needs access to the device node.
    pub fn descriptor(&self) -> Result<CcidDescriptor> {
        let device = self.device.open().wait()?;
        Ok(Layout::read(&device, self.interface)?.descriptor)
    }

    /// Open and claim this interface. See [`Reader::open`].
    pub fn open(&self) -> Result<Reader> {
        Reader::open(self)
    }
}

/// Descriptor and endpoint addresses of one CCID interface.
struct Layout {
    descriptor: CcidDescriptor,
    bulk_out: u8,
    bulk_in: u8,
    interrupt_in: Option<u8>,
}

impl Layout {
    fn read(device: &nusb::Device, interface: u8) -> Result<Layout> {
        let config = device
            .active_configuration()
            .map_err(|e| Error::Usb(e.into()))?;
        let alt = config
            .interface_alt_settings()
            .find(|a| a.interface_number() == interface && a.alternate_setting() == 0)
            .ok_or(Error::NotFound)?;
        // CCID 1.1 puts the class descriptor before the endpoints, but some
        // readers put it after them, so search everything under the interface.
        let raw = alt
            .descriptors()
            .find(|d| d.descriptor_type() == CCID_DESCRIPTOR_TYPE)
            .ok_or_else(|| {
                Error::Unsupported(format!(
                    "interface {interface} has no CCID class descriptor"
                ))
            })?;
        let descriptor = CcidDescriptor::parse(raw.as_bytes())?;

        let (mut bulk_out, mut bulk_in, mut interrupt_in) = (None, None, None);
        for ep in alt.endpoints() {
            let slot = match (ep.transfer_type(), ep.direction()) {
                (TransferType::Bulk, Direction::Out) => &mut bulk_out,
                (TransferType::Bulk, Direction::In) => &mut bulk_in,
                (TransferType::Interrupt, Direction::In) => &mut interrupt_in,
                _ => continue,
            };
            slot.get_or_insert(ep.address());
        }
        let (Some(bulk_out), Some(bulk_in)) = (bulk_out, bulk_in) else {
            return Err(Error::Unsupported(format!(
                "interface {interface} lacks a bulk IN or bulk OUT endpoint"
            )));
        };
        Ok(Layout {
            descriptor,
            bulk_out,
            bulk_in,
            interrupt_in,
        })
    }
}

/// An open CCID interface.
///
/// Commands run one at a time: every method takes `&mut self`. To share a
/// reader between tasks, wrap it in your runtime's mutex.
pub struct Reader {
    info: ReaderInfo,
    protocol: Protocol,
    /// Kept to open the interrupt endpoint for events.
    interface: nusb::Interface,
    endpoints: (u8, u8, Option<u8>),
}

impl Reader {
    /// Open the USB device and claim the CCID interface.
    ///
    /// Fails with [`Error::Unsupported`] if the reader does not work at an
    /// APDU exchange level, and with [`Error::Busy`] if another driver holds
    /// the interface.
    ///
    /// This makes a few short blocking system calls.
    pub fn open(info: &ReaderInfo) -> Result<Reader> {
        let device = info.device.open().wait()?;
        let layout = Layout::read(&device, info.interface)?;
        match layout.descriptor.exchange_level() {
            Some(ExchangeLevel::ShortApdu | ExchangeLevel::ExtendedApdu) => {}
            Some(level) => {
                return Err(Error::Unsupported(format!(
                    "{level} exchange level (only APDU level readers are supported)"
                )));
            }
            None => {
                return Err(Error::Unsupported(format!(
                    "undefined exchange level in dwFeatures {:?}",
                    layout.descriptor.features
                )));
            }
        }
        let interface = device.claim_interface(info.interface).wait()?;
        let bulk_out = interface.endpoint::<Bulk, Out>(layout.bulk_out)?;
        let bulk_in = interface.endpoint::<Bulk, In>(layout.bulk_in)?;
        let transport = UsbTransport::new(
            interface.clone(),
            bulk_out,
            bulk_in,
            layout.descriptor.max_message_length,
        );
        Ok(Reader {
            info: info.clone(),
            protocol: Protocol::new(Box::new(transport), layout.descriptor),
            interface,
            endpoints: (layout.bulk_out, layout.bulk_in, layout.interrupt_in),
        })
    }

    /// Open the first CCID interface with this vendor ID, product ID and
    /// interface number.
    pub fn open_by_id(vendor_id: u16, product_id: u16, interface: u8) -> Result<Reader> {
        let info = list_readers()?
            .into_iter()
            .find(|r| {
                r.vendor_id() == vendor_id
                    && r.product_id() == product_id
                    && r.interface == interface
            })
            .ok_or(Error::NotFound)?;
        Reader::open(&info)
    }

    /// The interface this reader was opened from.
    pub fn info(&self) -> &ReaderInfo {
        &self.info
    }

    /// The CCID class descriptor of this interface.
    pub fn descriptor(&self) -> &CcidDescriptor {
        self.protocol.descriptor()
    }

    /// Time to wait for each reply from the reader. Defaults to 3 seconds.
    /// A time extension request from the reader restarts the wait.
    pub fn timeout(&self) -> Duration {
        self.protocol.timeout
    }

    /// Set the time to wait for each reply from the reader.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.protocol.timeout = timeout;
    }

    /// Ask the reader whether a card is in `slot` and powered
    /// (PC_to_RDR_GetSlotStatus).
    pub async fn slot_status(&mut self, slot: u8) -> Result<IccStatus> {
        self.protocol.slot_status(slot).await
    }

    /// Power the card on and return its ATR (PC_to_RDR_IccPowerOn).
    ///
    /// Fails with [`Error::NoCard`] if the slot is empty.
    pub async fn power_on(&mut self, slot: u8) -> Result<Atr> {
        self.protocol.power_on(slot).await
    }

    /// Power the card off (PC_to_RDR_IccPowerOff).
    pub async fn power_off(&mut self, slot: u8) -> Result<()> {
        self.protocol.power_off(slot).await
    }

    /// Send a command APDU to the card and return the response APDU
    /// (PC_to_RDR_XfrBlock). The card must be powered on.
    ///
    /// Fails with [`Error::CardRemoved`] if the card left during the exchange.
    pub async fn transmit(&mut self, slot: u8, apdu: &[u8]) -> Result<Response> {
        let raw = self.protocol.transmit(slot, apdu).await?;
        Response::from_bytes(&raw)
    }

    /// Send a vendor command to the reader (PC_to_RDR_Escape) and return its
    /// answer. The meaning of the bytes depends on the reader.
    pub async fn escape(&mut self, slot: u8, command: &[u8]) -> Result<Vec<u8>> {
        self.protocol.escape(slot, command).await
    }

    /// Start receiving card arrival and removal events.
    ///
    /// The reader detects cards itself and reports changes on its interrupt
    /// endpoint; the host does not poll. The stream first reports the cards
    /// already present (from one GetSlotStatus per slot), then every change.
    ///
    /// Fails with [`Error::Unsupported`] if the interface has no interrupt
    /// endpoint (for example a SAM slot), and with
    /// [`Error::InvalidArgument`] if another [`SlotEvents`] of this reader
    /// is still alive.
    pub async fn events(&mut self) -> Result<SlotEvents> {
        let Some(address) = self.endpoints.2 else {
            return Err(Error::Unsupported(
                "interface has no interrupt endpoint, so it cannot report card events".into(),
            ));
        };
        let endpoint = self
            .interface
            .endpoint::<Interrupt, In>(address)
            .map_err(|e| match e.kind() {
                nusb::ErrorKind::Busy => {
                    Error::InvalidArgument("card events are already being read from this reader")
                }
                _ => Error::from(e),
            })?;
        // Listen before asking the status, so no change can fall in between.
        let slots = self.descriptor().slot_count();
        let mut events = SlotEvents::new(endpoint, slots);
        let mut present = Vec::with_capacity(slots);
        for slot in 0..slots {
            present.push(self.protocol.slot_status(slot as u8).await? != IccStatus::Absent);
        }
        events.set_initial(present);
        Ok(events)
    }

    /// Power the card in `slot` on and return a [`Card`] to talk to it.
    pub async fn connect(&mut self, slot: u8) -> Result<Card<'_>> {
        let atr = self.protocol.power_on(slot).await?;
        Ok(Card::new(&mut self.protocol, slot, atr))
    }
}

impl fmt::Debug for Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (bulk_out, bulk_in, interrupt_in) = self.endpoints;
        f.debug_struct("Reader")
            .field("interface", &self.info.interface)
            .field("bulk_out", &bulk_out)
            .field("bulk_in", &bulk_in)
            .field("interrupt_in", &interrupt_in)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The futures must be `Send` so callers can spawn them on a
    /// multi-threaded runtime. This only has to compile.
    #[test]
    fn async_api_is_send() {
        fn assert_send<T: Send>(_: T) {}
        fn reader(reader: &mut Reader) {
            assert_send(reader.slot_status(0));
            assert_send(reader.transmit(0, &[]));
            assert_send(reader.connect(0));
            assert_send(reader.events());
        }
        fn card(card: &mut Card<'_>) {
            assert_send(card.get_uid());
            assert_send(card.select_aid(&[]));
        }
        let _ = (reader, card);
    }
}
