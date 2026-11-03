use std::{fmt, io};

use crate::apdu::StatusWord;

/// Result type of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The reader was unplugged.
    #[error("reader disconnected")]
    Disconnected,

    /// The reader did not answer in time.
    #[error("timed out waiting for the reader")]
    Timeout,

    /// There is no card in the slot.
    #[error("no card in the slot")]
    NoCard,

    /// The card left the slot during the exchange.
    #[error("card removed during the exchange")]
    CardRemoved,

    /// The reader could not run the command and reported this slot error.
    #[error("reader error: {0}")]
    Slot(SlotError),

    /// The card answered with a status word other than `90 00`.
    ///
    /// Only returned by helpers that need a successful answer, such as
    /// [`Card::get_uid`](crate::Card::get_uid).
    #[error("card returned status {0}")]
    Status(StatusWord),

    /// Another driver holds the USB interface.
    ///
    /// On macOS this is usually `usbsmartcardreaderd`. On Linux it is
    /// usually pcscd.
    #[error(
        "USB interface is claimed by another driver (macOS: usbsmartcardreaderd, Linux: pcscd)"
    )]
    Busy,

    /// The OS refused access to the USB device.
    ///
    /// On Linux, install a udev rule for the reader.
    #[error("permission denied opening the USB device")]
    PermissionDenied,

    /// No matching reader or interface was found.
    #[error("reader not found")]
    NotFound,

    /// The reader does not support something this crate needs, such as the
    /// APDU exchange level.
    #[error("unsupported reader: {0}")]
    Unsupported(String),

    /// The reader sent data that does not follow the CCID specification.
    #[error("CCID protocol error: {0}")]
    Protocol(String),

    /// An argument is out of range, for example a slot number or an APDU
    /// that is too long for the reader.
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),

    /// Any other USB error.
    #[error("USB error: {0}")]
    Usb(#[source] io::Error),
}

impl From<nusb::Error> for Error {
    fn from(err: nusb::Error) -> Self {
        match err.kind() {
            nusb::ErrorKind::Disconnected => Error::Disconnected,
            nusb::ErrorKind::Busy => Error::Busy,
            nusb::ErrorKind::PermissionDenied => Error::PermissionDenied,
            nusb::ErrorKind::NotFound => Error::NotFound,
            _ => Error::Usb(err.into()),
        }
    }
}

/// `kIOReturnNotResponding`: macOS fails transfers with this code when the
/// device is unplugged while they are pending.
#[cfg(target_os = "macos")]
const IO_RETURN_NOT_RESPONDING: u32 = 0xE000_02ED;

impl From<nusb::transfer::TransferError> for Error {
    fn from(err: nusb::transfer::TransferError) -> Self {
        match err {
            nusb::transfer::TransferError::Disconnected => Error::Disconnected,
            #[cfg(target_os = "macos")]
            nusb::transfer::TransferError::Unknown(IO_RETURN_NOT_RESPONDING) => Error::Disconnected,
            nusb::transfer::TransferError::Cancelled => Error::Timeout,
            _ => Error::Usb(err.into()),
        }
    }
}

/// Slot error reported by the reader (`bError` of a failed command,
/// USB CCID 1.1 section 6.2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SlotError {
    /// The command was aborted (`CMD_ABORTED`).
    Aborted,
    /// The card did not answer (`ICC_MUTE`).
    IccMute,
    /// Parity error while talking to the card (`XFR_PARITY_ERROR`).
    ParityError,
    /// The card sent more data than expected (`XFR_OVERRUN`).
    Overrun,
    /// Hardware error in the reader (`HW_ERROR`).
    HardwareError,
    /// Bad TS byte in the ATR (`BAD_ATR_TS`).
    BadAtrTs,
    /// Bad TCK byte in the ATR (`BAD_ATR_TCK`).
    BadAtrTck,
    /// The card's protocol is not supported (`ICC_PROTOCOL_NOT_SUPPORTED`).
    ProtocolNotSupported,
    /// The card's voltage class is not supported (`ICC_CLASS_NOT_SUPPORTED`).
    ClassNotSupported,
    /// Conflict in T=0 procedure bytes (`PROCEDURE_BYTE_CONFLICT`).
    ProcedureByteConflict,
    /// The protocol was deactivated (`DEACTIVATED_PROTOCOL`).
    DeactivatedProtocol,
    /// The reader is busy with an automatic sequence (`BUSY_WITH_AUTO_SEQUENCE`).
    BusyWithAutoSequence,
    /// PIN entry timed out (`PIN_TIMEOUT`).
    PinTimeout,
    /// PIN entry was cancelled (`PIN_CANCELLED`).
    PinCancelled,
    /// The slot is busy with another command (`CMD_SLOT_BUSY`).
    SlotBusy,
    /// The reader does not support the command.
    CommandNotSupported,
    /// The command has a bad value at this byte offset of the message.
    BadParameter(u8),
    /// A code this crate does not know (RFU or vendor specific).
    Other(u8),
}

impl From<u8> for SlotError {
    fn from(code: u8) -> Self {
        match code {
            0xFF => SlotError::Aborted,
            0xFE => SlotError::IccMute,
            0xFD => SlotError::ParityError,
            0xFC => SlotError::Overrun,
            0xFB => SlotError::HardwareError,
            0xF8 => SlotError::BadAtrTs,
            0xF7 => SlotError::BadAtrTck,
            0xF6 => SlotError::ProtocolNotSupported,
            0xF5 => SlotError::ClassNotSupported,
            0xF4 => SlotError::ProcedureByteConflict,
            0xF3 => SlotError::DeactivatedProtocol,
            0xF2 => SlotError::BusyWithAutoSequence,
            0xF0 => SlotError::PinTimeout,
            0xEF => SlotError::PinCancelled,
            0xE0 => SlotError::SlotBusy,
            0x00 => SlotError::CommandNotSupported,
            0x01..=0x7F => SlotError::BadParameter(code),
            _ => SlotError::Other(code),
        }
    }
}

impl fmt::Display for SlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SlotError::Aborted => f.write_str("command aborted"),
            SlotError::IccMute => f.write_str("card mute"),
            SlotError::ParityError => f.write_str("parity error"),
            SlotError::Overrun => f.write_str("overrun"),
            SlotError::HardwareError => f.write_str("hardware error"),
            SlotError::BadAtrTs => f.write_str("bad ATR TS byte"),
            SlotError::BadAtrTck => f.write_str("bad ATR TCK byte"),
            SlotError::ProtocolNotSupported => f.write_str("card protocol not supported"),
            SlotError::ClassNotSupported => f.write_str("card voltage class not supported"),
            SlotError::ProcedureByteConflict => f.write_str("procedure byte conflict"),
            SlotError::DeactivatedProtocol => f.write_str("protocol deactivated"),
            SlotError::BusyWithAutoSequence => f.write_str("busy with automatic sequence"),
            SlotError::PinTimeout => f.write_str("PIN entry timed out"),
            SlotError::PinCancelled => f.write_str("PIN entry cancelled"),
            SlotError::SlotBusy => f.write_str("slot busy"),
            SlotError::CommandNotSupported => f.write_str("command not supported"),
            SlotError::BadParameter(offset) => write!(f, "bad parameter at offset {offset}"),
            SlotError::Other(code) => write!(f, "error code 0x{code:02x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use nusb::transfer::TransferError;

    use super::*;

    #[test]
    fn unplugged_transfers_map_to_disconnected() {
        assert!(matches!(
            Error::from(TransferError::Disconnected),
            Error::Disconnected
        ));
        #[cfg(target_os = "macos")]
        assert!(matches!(
            Error::from(TransferError::Unknown(0xE000_02ED)),
            Error::Disconnected
        ));
        assert!(matches!(Error::from(TransferError::Stall), Error::Usb(_)));
    }
}
