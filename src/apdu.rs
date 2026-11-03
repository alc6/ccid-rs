//! ISO/IEC 7816-4 APDUs.

use std::fmt;

use crate::{Error, Result};

/// Status word (`SW1 SW2`) at the end of a response APDU.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatusWord(pub u16);

impl StatusWord {
    /// `90 00`: normal processing.
    pub const SUCCESS: StatusWord = StatusWord(0x9000);

    /// Build a status word from its two bytes.
    pub fn new(sw1: u8, sw2: u8) -> StatusWord {
        StatusWord(u16::from_be_bytes([sw1, sw2]))
    }

    /// First byte.
    pub fn sw1(self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Second byte.
    pub fn sw2(self) -> u8 {
        self.0 as u8
    }

    /// Whether this is `90 00`.
    pub fn is_success(self) -> bool {
        self == StatusWord::SUCCESS
    }
}

impl fmt::Display for StatusWord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04X}", self.0)
    }
}

impl fmt::Debug for StatusWord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StatusWord({:04X})", self.0)
    }
}

/// A response APDU: data and status word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Response data, without the status word.
    pub data: Vec<u8>,
    /// Status word.
    pub sw: StatusWord,
}

impl Response {
    /// Split raw response bytes into data and status word.
    pub fn from_bytes(bytes: &[u8]) -> Result<Response> {
        let Some((data, sw)) = bytes.split_last_chunk::<2>() else {
            return Err(Error::Protocol(format!(
                "response APDU of {} bytes has no status word",
                bytes.len()
            )));
        };
        Ok(Response {
            data: data.to_vec(),
            sw: StatusWord::new(sw[0], sw[1]),
        })
    }

    /// The data if the status word is `90 00`, else [`Error::Status`].
    pub fn into_data(self) -> Result<Vec<u8>> {
        if self.sw.is_success() {
            Ok(self.data)
        } else {
            Err(Error::Status(self.sw))
        }
    }
}

/// Encode a command APDU. Uses the short form when the data and `le` fit,
/// else the extended form. `le` of 256 (short) or 65 536 (extended) is
/// encoded as 0.
pub(crate) fn command(header: [u8; 4], data: &[u8], le: Option<usize>) -> Result<Vec<u8>> {
    if data.len() > 65_535 || le.is_some_and(|le| le > 65_536) {
        return Err(Error::InvalidArgument("APDU data or Le too long"));
    }
    let short = data.len() <= 255 && le.is_none_or(|le| le <= 256);
    let mut apdu = Vec::with_capacity(4 + 3 + data.len() + 3);
    apdu.extend_from_slice(&header);
    if short {
        if !data.is_empty() {
            apdu.push(data.len() as u8);
            apdu.extend_from_slice(data);
        }
        if let Some(le) = le {
            apdu.push(le as u8); // 256 wraps to 0
        }
    } else {
        apdu.push(0);
        if !data.is_empty() {
            apdu.extend_from_slice(&(data.len() as u16).to_be_bytes());
            apdu.extend_from_slice(data);
        }
        if let Some(le) = le {
            apdu.extend_from_slice(&(le as u16).to_be_bytes()); // 65 536 wraps to 0
        }
    }
    Ok(apdu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_response() {
        let r = Response::from_bytes(&[0x01, 0x02, 0x6A, 0x81]).unwrap();
        assert_eq!(r.data, [0x01, 0x02]);
        assert_eq!(r.sw, StatusWord(0x6A81));
        assert!(matches!(
            r.into_data(),
            Err(Error::Status(StatusWord(0x6A81)))
        ));
        assert!(Response::from_bytes(&[0x90]).is_err());
    }

    #[test]
    fn encodes_short_and_extended_commands() {
        let select = [0x00, 0xA4, 0x04, 0x00];
        assert_eq!(command(select, &[], None).unwrap(), select);
        assert_eq!(
            command(select, &[0xA0, 0x00], Some(256)).unwrap(),
            [0x00, 0xA4, 0x04, 0x00, 0x02, 0xA0, 0x00, 0x00]
        );
        let long = vec![0x55; 300];
        let apdu = command(select, &long, Some(256)).unwrap();
        assert_eq!(&apdu[4..7], [0x00, 0x01, 0x2C]);
        assert_eq!(&apdu[apdu.len() - 2..], [0x01, 0x00]);
        assert_eq!(apdu.len(), 4 + 3 + 300 + 2);
    }
}
