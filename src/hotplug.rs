//! Waiting for a reader to be plugged in.

use std::time::{Duration, Instant};

use futures_lite::StreamExt;
use nusb::MaybeFuture;
use nusb::hotplug::HotplugEvent;

use crate::reader::CCID_CLASS;
use crate::{Error, ReaderInfo, Result, list_readers};

/// How long to look for the CCID interfaces of a device that just arrived.
/// The OS can list a device's interfaces a little after it reports the
/// device itself.
const SETTLE: Duration = Duration::from_secs(2);
const SETTLE_STEP: Duration = Duration::from_millis(50);

/// Wait until a CCID interface for which `filter` returns `true` is on the
/// bus, and return it. Returns at once if one is already there.
///
/// To wait for a reader that was just unplugged, use
/// [`ReaderInfo::wait_for_replug`] instead: right after the unplug, the OS
/// can still list the old connection for a short time.
pub async fn wait_for_reader(mut filter: impl FnMut(&ReaderInfo) -> bool) -> Result<ReaderInfo> {
    // Watch first, then list, so a reader plugged in between is not missed.
    let mut watch = nusb::watch_devices()?;
    if let Some(found) = list_readers()?.into_iter().find(|r| filter(r)) {
        return Ok(found);
    }
    while let Some(event) = watch.next().await {
        let HotplugEvent::Connected(device) = event else {
            continue;
        };
        let mut interfaces = device.interfaces().peekable();
        if interfaces.peek().is_some() && !interfaces.any(|i| i.class() == CCID_CLASS) {
            continue; // fully listed, and not a smart-card reader
        }
        let deadline = Instant::now() + SETTLE;
        loop {
            if let Some(found) = list_readers()?.into_iter().find(|r| filter(r)) {
                return Ok(found);
            }
            if Instant::now() >= deadline {
                break;
            }
            futures_timer::Delay::new(SETTLE_STEP).await;
        }
    }
    Err(Error::NotFound)
}

impl ReaderInfo {
    /// Wait until this reader is plugged in again, and return the new
    /// connection. Call it after a command or the event stream failed with
    /// [`Error::Disconnected`].
    ///
    /// The OS can report the error a little before it removes the device,
    /// so this first waits up to 2 seconds for the old connection to leave
    /// the bus. If it does not leave, the reader was not unplugged, and this
    /// returns it as it is. Otherwise it waits, with no time limit, until the
    /// same interface of the same reader comes back (see
    /// [`same_interface_as`](Self::same_interface_as)).
    ///
    /// Opening can fail for a short time right after the reader is back: on
    /// Linux, udev applies permissions a moment later
    /// ([`Error::PermissionDenied`]); on macOS, `usbsmartcardreaderd` claims
    /// the interface ([`Error::Busy`]). Retry [`ReaderInfo::open`] for a
    /// second or two.
    ///
    /// ```no_run
    /// # async fn demo(reader: ccid::Reader) -> Result<(), ccid::Error> {
    /// let info = reader.info().clone();
    /// drop(reader);
    /// let info = info.wait_for_replug().await?;
    /// let reader = info.open()?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn wait_for_replug(&self) -> Result<ReaderInfo> {
        let deadline = Instant::now() + SETTLE;
        while self.is_on_bus()? {
            if Instant::now() >= deadline {
                return Ok(self.clone());
            }
            futures_timer::Delay::new(SETTLE_STEP).await;
        }
        wait_for_reader(|r| r.same_interface_as(self)).await
    }

    /// Whether this connection of the device is still listed by the OS.
    fn is_on_bus(&self) -> Result<bool> {
        let id = self.device.id();
        Ok(nusb::list_devices().wait()?.any(|d| d.id() == id))
    }
}
