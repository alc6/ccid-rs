//! Reader discovery and opening.

use std::fmt;

use nusb::MaybeFuture;
use nusb::descriptors::TransferType;
use nusb::transfer::{Bulk, Direction, In, Out};

use crate::descriptor::{CCID_DESCRIPTOR_TYPE, CcidDescriptor, ExchangeLevel};
use crate::{Error, Result};

/// USB interface class of CCID readers.
const CCID_CLASS: u8 = 0x0B;

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
    device: nusb::DeviceInfo,
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
pub struct Reader {
    info: ReaderInfo,
    descriptor: CcidDescriptor,
    interface: nusb::Interface,
    bulk_out: nusb::Endpoint<Bulk, Out>,
    bulk_in: nusb::Endpoint<Bulk, In>,
    interrupt_in: Option<u8>,
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
        Ok(Reader {
            info: info.clone(),
            descriptor: layout.descriptor,
            interface,
            bulk_out,
            bulk_in,
            interrupt_in: layout.interrupt_in,
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
        &self.descriptor
    }
}

impl fmt::Debug for Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reader")
            .field("interface", &self.interface.interface_number())
            .field("bulk_out", &self.bulk_out.endpoint_address())
            .field("bulk_in", &self.bulk_in.endpoint_address())
            .field("interrupt_in", &self.interrupt_in)
            .finish_non_exhaustive()
    }
}
