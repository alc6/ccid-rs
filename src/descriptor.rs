//! The CCID class descriptor (USB CCID 1.1, section 5.1).

use std::fmt;

use crate::{Error, Result};

/// `bDescriptorType` of the CCID class descriptor.
pub(crate) const CCID_DESCRIPTOR_TYPE: u8 = 0x21;

/// Size of the CCID class descriptor in CCID 1.0 and 1.1.
const CCID_DESCRIPTOR_LEN: usize = 54;

/// Mask of the exchange level bits in `dwFeatures`.
const EXCHANGE_LEVEL_MASK: u32 = 0x0007_0000;

/// The CCID class descriptor of one reader interface.
///
/// Field names follow the specification, without the type prefix
/// (`dwMaxCCIDMessageLength` becomes `max_message_length`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CcidDescriptor {
    /// CCID release number in BCD (`bcdCCID`), `0x0110` for 1.1.
    pub bcd_ccid: u16,
    /// Index of the highest slot (`bMaxSlotIndex`). Slots are numbered from 0.
    pub max_slot_index: u8,
    /// Card voltages the reader can supply (`bVoltageSupport`).
    pub voltages: Voltages,
    /// Card protocols the reader supports (`dwProtocols`).
    pub protocols: Protocols,
    /// Default card clock in kHz (`dwDefaultClock`).
    pub default_clock_khz: u32,
    /// Maximum card clock in kHz (`dwMaximumClock`).
    pub max_clock_khz: u32,
    /// Number of clock frequencies the reader supports (`bNumClockSupported`).
    pub num_clocks: u8,
    /// Default data rate in bit/s (`dwDataRate`).
    pub data_rate: u32,
    /// Maximum data rate in bit/s (`dwMaxDataRate`).
    pub max_data_rate: u32,
    /// Number of data rates the reader supports (`bNumDataRatesSupported`).
    pub num_data_rates: u8,
    /// Maximum IFSD for T=1 (`dwMaxIFSD`).
    pub max_ifsd: u32,
    /// Synchronous protocols (`dwSynchProtocols`).
    pub synch_protocols: u32,
    /// Mechanical features (`dwMechanical`).
    pub mechanical: u32,
    /// Reader features (`dwFeatures`), including the exchange level.
    pub features: Features,
    /// Largest message the reader accepts, header included
    /// (`dwMaxCCIDMessageLength`).
    pub max_message_length: u32,
    /// Class byte the reader uses for GET RESPONSE (`bClassGetResponse`).
    pub class_get_response: u8,
    /// Class byte the reader uses for ENVELOPE (`bClassEnvelope`).
    pub class_envelope: u8,
    /// LCD layout (`wLcdLayout`), 0 when the reader has no LCD.
    pub lcd_layout: u16,
    /// PIN support (`bPINSupport`).
    pub pin_support: u8,
    /// Number of slots that can be busy at the same time (`bMaxCCIDBusySlots`).
    pub max_busy_slots: u8,
}

impl CcidDescriptor {
    /// Parse a CCID class descriptor, starting at its `bLength` byte.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let len = bytes.first().copied().unwrap_or(0) as usize;
        if len < CCID_DESCRIPTOR_LEN || bytes.len() < CCID_DESCRIPTOR_LEN {
            return Err(Error::Protocol(format!(
                "CCID class descriptor is {} bytes, expected {CCID_DESCRIPTOR_LEN}",
                len.min(bytes.len())
            )));
        }
        if bytes[1] != CCID_DESCRIPTOR_TYPE {
            return Err(Error::Protocol(format!(
                "descriptor type 0x{:02x} is not a CCID class descriptor",
                bytes[1]
            )));
        }
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        Ok(CcidDescriptor {
            bcd_ccid: u16_at(2),
            max_slot_index: bytes[4],
            voltages: Voltages(bytes[5]),
            protocols: Protocols(u32_at(6)),
            default_clock_khz: u32_at(10),
            max_clock_khz: u32_at(14),
            num_clocks: bytes[18],
            data_rate: u32_at(19),
            max_data_rate: u32_at(23),
            num_data_rates: bytes[27],
            max_ifsd: u32_at(28),
            synch_protocols: u32_at(32),
            mechanical: u32_at(36),
            features: Features(u32_at(40)),
            max_message_length: u32_at(44),
            class_get_response: bytes[48],
            class_envelope: bytes[49],
            lcd_layout: u16_at(50),
            pin_support: bytes[52],
            max_busy_slots: bytes[53],
        })
    }

    /// Exchange level declared in `dwFeatures`, or `None` if the bits hold
    /// a value the specification does not define.
    pub fn exchange_level(&self) -> Option<ExchangeLevel> {
        self.features.exchange_level()
    }

    /// Number of slots on this interface.
    pub fn slot_count(&self) -> usize {
        usize::from(self.max_slot_index) + 1
    }
}

/// How the reader exchanges data with the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExchangeLevel {
    /// Character level: the host sends single characters. Not supported.
    Character,
    /// TPDU level: the host runs the T=0 or T=1 protocol. Not supported.
    Tpdu,
    /// Short APDU level: the reader takes complete short APDUs.
    ShortApdu,
    /// Short and extended APDU level: the reader also takes extended APDUs,
    /// split over several messages if they exceed `max_message_length`.
    ExtendedApdu,
}

impl fmt::Display for ExchangeLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ExchangeLevel::Character => "character",
            ExchangeLevel::Tpdu => "TPDU",
            ExchangeLevel::ShortApdu => "short APDU",
            ExchangeLevel::ExtendedApdu => "short and extended APDU",
        })
    }
}

/// The `dwFeatures` bit field.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Features(pub u32);

/// Single-bit features, with a short name for display.
const FEATURE_NAMES: &[(u32, &str)] = &[
    (0x0000_0002, "auto parameters from ATR"),
    (0x0000_0004, "auto activation on insertion"),
    (0x0000_0008, "auto voltage selection"),
    (0x0000_0010, "auto clock change"),
    (0x0000_0020, "auto baud rate change"),
    (0x0000_0040, "auto parameter negotiation"),
    (0x0000_0080, "auto PPS"),
    (0x0000_0100, "clock stop"),
    (0x0000_0200, "NAD other than 0"),
    (0x0000_0400, "auto IFSD exchange"),
    (0x0010_0000, "USB wake-up on card event"),
];

impl Features {
    /// Exchange level, or `None` if the bits hold an undefined value.
    pub fn exchange_level(self) -> Option<ExchangeLevel> {
        match self.0 & EXCHANGE_LEVEL_MASK {
            0x0000_0000 => Some(ExchangeLevel::Character),
            0x0001_0000 => Some(ExchangeLevel::Tpdu),
            0x0002_0000 => Some(ExchangeLevel::ShortApdu),
            0x0004_0000 => Some(ExchangeLevel::ExtendedApdu),
            _ => None,
        }
    }

    /// Short names of the single-bit features that are set.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        FEATURE_NAMES
            .iter()
            .filter(move |(bit, _)| self.0 & bit != 0)
            .map(|(_, name)| *name)
    }
}

impl fmt::Debug for Features {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Features(0x{:08x}", self.0)?;
        for name in self.names() {
            write!(f, ", {name}")?;
        }
        match self.exchange_level() {
            Some(level) => write!(f, ", {level} level)"),
            None => write!(f, ", undefined exchange level)"),
        }
    }
}

/// The `bVoltageSupport` bit field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Voltages(pub u8);

impl Voltages {
    /// The reader can supply 5.0 V.
    pub fn v5_0(self) -> bool {
        self.0 & 0x01 != 0
    }
    /// The reader can supply 3.0 V.
    pub fn v3_0(self) -> bool {
        self.0 & 0x02 != 0
    }
    /// The reader can supply 1.8 V.
    pub fn v1_8(self) -> bool {
        self.0 & 0x04 != 0
    }
}

impl fmt::Display for Voltages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (self.v5_0(), "5.0V"),
            (self.v3_0(), "3.0V"),
            (self.v1_8(), "1.8V"),
        ];
        let set: Vec<_> = names
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| *n)
            .collect();
        f.write_str(&set.join(" "))
    }
}

/// The `dwProtocols` bit field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Protocols(pub u32);

impl Protocols {
    /// The reader supports T=0.
    pub fn t0(self) -> bool {
        self.0 & 0x01 != 0
    }
    /// The reader supports T=1.
    pub fn t1(self) -> bool {
        self.0 & 0x02 != 0
    }
}

impl fmt::Display for Protocols {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [(self.t0(), "T=0"), (self.t1(), "T=1")];
        let set: Vec<_> = names
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| *n)
            .collect();
        f.write_str(&set.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Class descriptor of a contactless reader interface (ACR1552U PICC),
    /// as read from the device.
    const PICC: [u8; 54] = [
        0x36, 0x21, 0x10, 0x01, 0x00, 0x07, 0x03, 0x00, 0x00, 0x00, 0x88, 0x13, 0x00, 0x00, 0xF8,
        0x34, 0x00, 0x00, 0x00, 0xD2, 0x9D, 0x01, 0x00, 0x8C, 0xEE, 0x0C, 0x00, 0x00, 0xFA, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBA, 0x04, 0x04, 0x00, 0x04,
        0x02, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01,
    ];

    #[test]
    fn parses_reader_descriptor() {
        let d = CcidDescriptor::parse(&PICC).unwrap();
        assert_eq!(d.bcd_ccid, 0x0110);
        assert_eq!(d.slot_count(), 1);
        assert!(d.voltages.v5_0() && d.voltages.v3_0() && d.voltages.v1_8());
        assert!(d.protocols.t0() && d.protocols.t1());
        assert_eq!(d.default_clock_khz, 5000);
        assert_eq!(d.max_clock_khz, 13560);
        assert_eq!(d.data_rate, 105_938);
        assert_eq!(d.max_data_rate, 847_500);
        assert_eq!(d.max_ifsd, 506);
        assert_eq!(d.features.0, 0x0004_04BA);
        assert_eq!(d.exchange_level(), Some(ExchangeLevel::ExtendedApdu));
        assert_eq!(d.max_message_length, 516);
        assert_eq!((d.class_get_response, d.class_envelope), (0xFF, 0xFF));
        assert_eq!(d.max_busy_slots, 1);
    }

    #[test]
    fn decodes_every_exchange_level() {
        let level = |bits| Features(bits).exchange_level();
        assert_eq!(level(0x0000_0000), Some(ExchangeLevel::Character));
        assert_eq!(level(0x0001_0000), Some(ExchangeLevel::Tpdu));
        assert_eq!(level(0x0002_0000), Some(ExchangeLevel::ShortApdu));
        assert_eq!(level(0x0004_0000), Some(ExchangeLevel::ExtendedApdu));
        assert_eq!(level(0x0003_0000), None);
    }

    #[test]
    fn rejects_bad_descriptors() {
        assert!(CcidDescriptor::parse(&PICC[..53]).is_err());
        let mut wrong_type = PICC;
        wrong_type[1] = 0x24;
        assert!(CcidDescriptor::parse(&wrong_type).is_err());
        let mut short_length = PICC;
        short_length[0] = 9;
        assert!(CcidDescriptor::parse(&short_length).is_err());
    }
}
