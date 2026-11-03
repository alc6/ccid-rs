use std::io;

/// Result type of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The reader was unplugged.
    #[error("reader disconnected")]
    Disconnected,

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
