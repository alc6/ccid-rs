//! Moves CCID messages between the protocol layer and the USB pipes.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use nusb::transfer::{Buffer, Bulk, ControlOut, ControlType, In, Out, Recipient};

use crate::{Error, Result};

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The bulk pipes and the control pipe of one CCID interface.
///
/// The protocol layer only talks to this trait, so tests can replace the
/// USB device with a scripted fake.
pub(crate) trait Transport: Send {
    /// Send one complete message on the bulk OUT pipe.
    fn write<'a>(&'a mut self, msg: &'a [u8], timeout: Duration) -> BoxFuture<'a, Result<()>>;

    /// Receive one bulk IN transfer. It may hold only part of a message, or
    /// nothing at all (a zero-length packet).
    fn read(&mut self, timeout: Duration) -> BoxFuture<'_, Result<Vec<u8>>>;

    /// Send the ABORT class request (CCID 1.1, section 5.3.1).
    fn abort(&mut self, slot: u8, seq: u8) -> BoxFuture<'_, Result<()>>;
}

/// Run `fut`, or give up after `after`.
pub(crate) async fn with_timeout<F: Future>(fut: F, after: Duration) -> Option<F::Output> {
    futures_lite::future::or(async { Some(fut.await) }, async {
        futures_timer::Delay::new(after).await;
        None
    })
    .await
}

/// [`Transport`] over a claimed nusb interface.
pub(crate) struct UsbTransport {
    interface: nusb::Interface,
    bulk_out: nusb::Endpoint<Bulk, Out>,
    bulk_in: nusb::Endpoint<Bulk, In>,
    /// Size of each bulk IN request: the largest message the reader sends,
    /// rounded up to whole packets.
    read_len: usize,
}

impl UsbTransport {
    pub(crate) fn new(
        interface: nusb::Interface,
        bulk_out: nusb::Endpoint<Bulk, Out>,
        bulk_in: nusb::Endpoint<Bulk, In>,
        max_message_length: u32,
    ) -> UsbTransport {
        let packet = bulk_in.max_packet_size().max(1);
        let wanted = (max_message_length as usize).max(packet);
        let read_len = wanted.div_ceil(packet) * packet;
        UsbTransport {
            interface,
            bulk_out,
            bulk_in,
            read_len,
        }
    }
}

impl Transport for UsbTransport {
    fn write<'a>(&'a mut self, msg: &'a [u8], timeout: Duration) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.bulk_out.submit(Buffer::from(msg));
            let done = with_timeout(self.bulk_out.next_complete(), timeout).await;
            let completion = match done {
                Some(c) => c,
                None => {
                    self.bulk_out.cancel_all();
                    self.bulk_out.next_complete().await;
                    return Err(Error::Timeout);
                }
            };
            completion.status.map_err(Error::from)
        })
    }

    fn read(&mut self, timeout: Duration) -> BoxFuture<'_, Result<Vec<u8>>> {
        Box::pin(async move {
            self.bulk_in.submit(Buffer::new(self.read_len));
            let done = with_timeout(self.bulk_in.next_complete(), timeout).await;
            let completion = match done {
                Some(c) => c,
                None => {
                    // Wait for the cancelled transfer to come back so the
                    // endpoint is idle. It may have completed in the meantime.
                    self.bulk_in.cancel_all();
                    let c = self.bulk_in.next_complete().await;
                    if c.status.is_ok() {
                        return Ok(c.buffer.into_vec());
                    }
                    return Err(Error::Timeout);
                }
            };
            completion.status?;
            Ok(completion.buffer.into_vec())
        })
    }

    fn abort(&mut self, slot: u8, seq: u8) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let request = ControlOut {
                control_type: ControlType::Class,
                recipient: Recipient::Interface,
                request: 0x01, // ABORT
                value: u16::from_be_bytes([seq, slot]),
                index: u16::from(self.interface.interface_number()),
                data: &[],
            };
            self.interface
                .control_out(request, Duration::from_secs(1))
                .await
                .map_err(Error::from)
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A scripted transport for tests.

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// What the fake saw and what it will answer.
    #[derive(Default)]
    pub(crate) struct Script {
        /// Answers to `read`, in order. When empty, `read` times out.
        pub reads: VecDeque<Result<Vec<u8>>>,
        /// Messages passed to `write`.
        pub writes: Vec<Vec<u8>>,
        /// `(slot, seq)` of each ABORT request.
        pub aborts: Vec<(u8, u8)>,
    }

    #[derive(Clone, Default)]
    pub(crate) struct FakeTransport(pub Arc<Mutex<Script>>);

    impl FakeTransport {
        /// Queue one bulk IN transfer.
        pub(crate) fn push_read(&self, chunk: &[u8]) {
            self.0.lock().unwrap().reads.push_back(Ok(chunk.to_vec()));
        }

        /// Queue an error from `read`.
        pub(crate) fn push_read_error(&self, err: Error) {
            self.0.lock().unwrap().reads.push_back(Err(err));
        }

        pub(crate) fn writes(&self) -> Vec<Vec<u8>> {
            self.0.lock().unwrap().writes.clone()
        }

        pub(crate) fn aborts(&self) -> Vec<(u8, u8)> {
            self.0.lock().unwrap().aborts.clone()
        }
    }

    impl Transport for FakeTransport {
        fn write<'a>(&'a mut self, msg: &'a [u8], _: Duration) -> BoxFuture<'a, Result<()>> {
            self.0.lock().unwrap().writes.push(msg.to_vec());
            Box::pin(async { Ok(()) })
        }

        fn read(&mut self, _: Duration) -> BoxFuture<'_, Result<Vec<u8>>> {
            let next = self.0.lock().unwrap().reads.pop_front();
            Box::pin(async move { next.unwrap_or(Err(Error::Timeout)) })
        }

        fn abort(&mut self, slot: u8, seq: u8) -> BoxFuture<'_, Result<()>> {
            self.0.lock().unwrap().aborts.push((slot, seq));
            Box::pin(async { Ok(()) })
        }
    }
}
