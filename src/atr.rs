//! Answer To Reset: ISO/IEC 7816-3 structure and the contactless format of
//! PC/SC Part 3.

use std::fmt;
use std::ops::Range;

/// Registered application provider ID of the PC/SC Workgroup, used in the
/// ATR of contactless storage cards.
const PCSC_RID: [u8; 5] = [0xA0, 0x00, 0x00, 0x03, 0x06];

/// Answer To Reset of a card.
#[derive(Clone, PartialEq, Eq)]
pub struct Atr {
    bytes: Vec<u8>,
    /// Position of the historical bytes, `None` if the ATR is malformed.
    historical: Option<Range<usize>>,
}

impl Atr {
    /// Parse ATR bytes. This never fails: if the structure is malformed,
    /// [`historical_bytes`](Self::historical_bytes) is empty and
    /// [`card_type`](Self::card_type) is [`CardType::Unknown`].
    pub fn new(bytes: Vec<u8>) -> Atr {
        let historical = historical_range(&bytes);
        Atr { bytes, historical }
    }

    /// Raw ATR bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Historical bytes. For contactless ISO 14443-4 cards these come from
    /// the card's ATS (type A) or ATQB (type B).
    pub fn historical_bytes(&self) -> &[u8] {
        self.historical.clone().map_or(&[], |r| &self.bytes[r])
    }

    /// Card type, decoded from the contactless ATR format of PC/SC Part 3.
    ///
    /// See the PC/SC Part 3 supplemental document for the standard and card
    /// name values:
    /// <https://pcscworkgroup.com/Download/Specifications/pcsc3_v2.01.09_sup.pdf>
    pub fn card_type(&self) -> CardType {
        // Contactless ATRs built by the reader: 3B 8n 80 01 <historical> TCK.
        let b = &self.bytes;
        if self.historical.is_none()
            || b.len() < 4
            || b[0] != 0x3B
            || b[1] & 0xF0 != 0x80
            || b[2] != 0x80
            || b[3] != 0x01
        {
            return CardType::Unknown;
        }
        match self.historical_bytes() {
            // Storage card: category 80, AID tag 4F, length 0C, RID, SS, NN NN, 4 RFU.
            [0x80, 0x4F, 0x0C, rid @ .., standard, n1, n2, _, _, _, _] if *rid == PCSC_RID => {
                storage_card(*standard, u16::from_be_bytes([*n1, *n2]))
            }
            _ => CardType::Iso14443_4,
        }
    }
}

impl fmt::Display for Atr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.bytes {
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Atr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Atr({self})")
    }
}

/// Find the historical bytes by walking the interface bytes
/// (ISO/IEC 7816-3, section 8.2).
fn historical_range(atr: &[u8]) -> Option<Range<usize>> {
    let t0 = *atr.get(1)?;
    let historical_len = usize::from(t0 & 0x0F);
    let mut indicator = t0 >> 4; // which of TA, TB, TC, TD follow
    let mut pos = 2;
    let mut needs_tck = false;
    loop {
        // TA, TB and TC are skipped; TD says what comes next.
        pos += (indicator & 0x07).count_ones() as usize;
        if indicator & 0x08 == 0 {
            break;
        }
        let td = *atr.get(pos)?;
        pos += 1;
        // TCK is present when any protocol other than T=0 is offered.
        needs_tck |= td & 0x0F != 0;
        indicator = td >> 4;
    }
    let end = pos + historical_len;
    let expected_len = end + usize::from(needs_tck);
    (atr.len() >= expected_len).then_some(pos..end)
}

/// Card type decoded from the ATR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CardType {
    /// MIFARE Classic 1K.
    MifareClassic1K,
    /// MIFARE Classic 4K.
    MifareClassic4K,
    /// MIFARE Mini.
    MifareMini,
    /// MIFARE Ultralight.
    MifareUltralight,
    /// MIFARE Ultralight C.
    MifareUltralightC,
    /// MIFARE Ultralight EV1.
    MifareUltralightEv1,
    /// MIFARE Plus at security level 1 or 2.
    MifarePlus {
        /// Security level (1 or 2).
        security_level: u8,
        /// Memory size in KiB (2 or 4).
        kib: u8,
    },
    /// FeliCa.
    Felica,
    /// Jewel tag.
    Jewel,
    /// Topaz NFC tag.
    Topaz,
    /// ICODE SLI.
    IcodeSli,
    /// ICODE SLIX / SL2.
    IcodeSl2,
    /// Another storage card, with the raw PC/SC standard byte and card name.
    OtherStorage {
        /// Standard byte (`SS`), for example `0x03` for ISO 14443 A part 3.
        standard: u8,
        /// Card name (`NN NN`).
        name: u16,
    },
    /// An ISO 14443-4 card (T=CL), such as a payment card.
    Iso14443_4,
    /// The ATR is not in the PC/SC contactless format (for example a
    /// contact card).
    Unknown,
}

fn storage_card(standard: u8, name: u16) -> CardType {
    match name {
        0x0001 => CardType::MifareClassic1K,
        0x0002 => CardType::MifareClassic4K,
        0x0003 => CardType::MifareUltralight,
        0x0014 => CardType::IcodeSli,
        0x0026 => CardType::MifareMini,
        0x002F => CardType::Jewel,
        0x0030 => CardType::Topaz,
        0x0035 => CardType::IcodeSl2,
        0x0036 => CardType::MifarePlus {
            security_level: 1,
            kib: 2,
        },
        0x0037 => CardType::MifarePlus {
            security_level: 1,
            kib: 4,
        },
        0x0038 => CardType::MifarePlus {
            security_level: 2,
            kib: 2,
        },
        0x0039 => CardType::MifarePlus {
            security_level: 2,
            kib: 4,
        },
        0x003A => CardType::MifareUltralightC,
        0x003B => CardType::Felica,
        0x003D => CardType::MifareUltralightEv1,
        // Standard byte 0x11 is FeliCa, whatever the card name.
        _ if standard == 0x11 => CardType::Felica,
        _ => CardType::OtherStorage { standard, name },
    }
}

impl fmt::Display for CardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardType::MifareClassic1K => f.write_str("MIFARE Classic 1K"),
            CardType::MifareClassic4K => f.write_str("MIFARE Classic 4K"),
            CardType::MifareMini => f.write_str("MIFARE Mini"),
            CardType::MifareUltralight => f.write_str("MIFARE Ultralight"),
            CardType::MifareUltralightC => f.write_str("MIFARE Ultralight C"),
            CardType::MifareUltralightEv1 => f.write_str("MIFARE Ultralight EV1"),
            CardType::MifarePlus {
                security_level,
                kib,
            } => write!(f, "MIFARE Plus {kib}K SL{security_level}"),
            CardType::Felica => f.write_str("FeliCa"),
            CardType::Jewel => f.write_str("Jewel"),
            CardType::Topaz => f.write_str("Topaz"),
            CardType::IcodeSli => f.write_str("ICODE SLI"),
            CardType::IcodeSl2 => f.write_str("ICODE SLIX"),
            CardType::OtherStorage { standard, name } => {
                write!(
                    f,
                    "storage card (standard 0x{standard:02X}, name 0x{name:04X})"
                )
            }
            CardType::Iso14443_4 => f.write_str("ISO 14443-4"),
            CardType::Unknown => f.write_str("unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atr(hex: &str) -> Atr {
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        Atr::new(bytes)
    }

    #[test]
    fn decodes_card_types() {
        // Storage cards: 3B 8F 80 01 80 4F 0C <RID> SS NN NN 00 00 00 00 TCK.
        let classic_1k = atr("3B8F8001804F0CA000000306030001000000006A");
        assert_eq!(classic_1k.card_type(), CardType::MifareClassic1K);
        assert_eq!(
            classic_1k.to_string(),
            "3B8F8001804F0CA000000306030001000000006A"
        );
        let ultralight = atr("3B8F8001804F0CA0000003060300030000000068");
        assert_eq!(ultralight.card_type(), CardType::MifareUltralight);
        let felica = atr("3B8F8001804F0CA0000003061100F00000000089");
        assert_eq!(felica.card_type(), CardType::Felica);
        let unnamed = atr("3B8F8001804F0CA0000003060B00FF000000009C");
        assert_eq!(
            unnamed.card_type(),
            CardType::OtherStorage {
                standard: 0x0B,
                name: 0x00FF
            }
        );
        // ISO 14443-4 card without historical bytes.
        let bank = atr("3B80800101");
        assert_eq!(bank.card_type(), CardType::Iso14443_4);
        assert!(bank.historical_bytes().is_empty());
        // Contact card (T=1 SAM) is not in the contactless format.
        let sam = atr("3BF89700008131FE4541434F530604214019");
        assert_eq!(sam.card_type(), CardType::Unknown);
        assert_eq!(sam.historical_bytes(), b"ACOS\x06\x04\x21\x40");
    }

    #[test]
    fn tolerates_malformed_atrs() {
        for bad in [&[][..], &[0x3B], &[0x3B, 0x8F, 0x80, 0x01, 0x80]] {
            let atr = Atr::new(bad.to_vec());
            assert!(atr.historical_bytes().is_empty());
            assert_eq!(atr.card_type(), CardType::Unknown);
        }
    }
}
