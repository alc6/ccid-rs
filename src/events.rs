//! Card arrival and removal events from the interrupt pipe.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_lite::Stream;
use nusb::transfer::{Buffer, In, Interrupt, TransferError};

use crate::message::{Notification, parse_notifications};
use crate::{Error, Result};

/// Interrupt transfers kept in flight, so a notification that arrives while
/// the caller is busy is not lost.
const TRANSFERS: usize = 2;

/// A change in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotEvent {
    /// Slot number.
    pub slot: u8,
    /// What happened.
    pub kind: SlotEventKind,
}

/// What happened in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SlotEventKind {
    /// A card is in the slot. For a contactless reader: a card entered the
    /// field and the reader activated it.
    CardPresent,
    /// The card left the slot.
    CardRemoved,
    /// The reader reported a hardware error for the slot
    /// (RDR_to_PC_HardwareError), with its code. `0x01` is overcurrent.
    HardwareError(u8),
}

/// Stream of [`SlotEvent`]s, from [`Reader::events`](crate::Reader::events).
///
/// It starts with a [`CardPresent`](SlotEventKind::CardPresent) event for
/// each slot that already holds a card, then reports every change the
/// reader notifies. It ends after yielding [`Error::Disconnected`] when the
/// reader is unplugged.
///
/// The stream does not borrow the [`Reader`](crate::Reader), so the reader
/// can talk to the card while the stream is alive.
pub struct SlotEvents {
    endpoint: nusb::Endpoint<Interrupt, In>,
    slots: usize,
    tracker: SlotTracker,
    queue: VecDeque<SlotEvent>,
    done: bool,
}

impl SlotEvents {
    /// Start reading the interrupt pipe. Call [`Self::set_initial`] before
    /// handing the stream out.
    pub(crate) fn new(mut endpoint: nusb::Endpoint<Interrupt, In>, slots: usize) -> SlotEvents {
        let len = endpoint.max_packet_size().max(1);
        for _ in 0..TRANSFERS {
            endpoint.submit(Buffer::new(len));
        }
        SlotEvents {
            endpoint,
            slots,
            tracker: SlotTracker::new(vec![false; slots]),
            queue: VecDeque::new(),
            done: false,
        }
    }

    /// Record which slots hold a card now, and queue a `CardPresent` event
    /// for each of them.
    pub(crate) fn set_initial(&mut self, present: Vec<bool>) {
        for (slot, _) in present.iter().enumerate().filter(|(_, p)| **p) {
            self.queue.push_back(SlotEvent {
                slot: slot as u8,
                kind: SlotEventKind::CardPresent,
            });
        }
        self.tracker = SlotTracker::new(present);
    }
}

impl Stream for SlotEvents {
    type Item = Result<SlotEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(event) = this.queue.pop_front() {
                return Poll::Ready(Some(Ok(event)));
            }
            if this.done {
                return Poll::Ready(None);
            }
            let completion = match this.endpoint.poll_next_complete(cx) {
                Poll::Ready(c) => c,
                Poll::Pending => return Poll::Pending,
            };
            match completion.status {
                Ok(()) => {
                    log::trace!("interrupt {:02X?}", &completion.buffer[..]);
                    for notification in parse_notifications(&completion.buffer, this.slots) {
                        this.queue.extend(this.tracker.apply(&notification));
                    }
                    let mut buffer = completion.buffer;
                    buffer.clear();
                    this.endpoint.submit(buffer);
                }
                Err(err) => {
                    this.done = true;
                    this.endpoint.cancel_all();
                    if err == TransferError::Cancelled {
                        return Poll::Ready(None);
                    }
                    return Poll::Ready(Some(Err(Error::from(err))));
                }
            }
        }
    }
}

impl std::fmt::Debug for SlotEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotEvents")
            .field("endpoint", &self.endpoint.endpoint_address())
            .field("queued", &self.queue.len())
            .finish_non_exhaustive()
    }
}

/// Turns notifications into events, remembering which slots hold a card.
struct SlotTracker {
    present: Vec<bool>,
    /// Whether a SlotChange notification was applied since the initial state.
    synced: bool,
}

impl SlotTracker {
    fn new(present: Vec<bool>) -> SlotTracker {
        SlotTracker {
            present,
            synced: false,
        }
    }

    fn apply(&mut self, notification: &Notification) -> Vec<SlotEvent> {
        let mut events = Vec::new();
        let mut push = |slot: usize, present: bool| {
            events.push(SlotEvent {
                slot: slot as u8,
                kind: if present {
                    SlotEventKind::CardPresent
                } else {
                    SlotEventKind::CardRemoved
                },
            });
        };
        match notification {
            Notification::SlotChange(bitmap) => {
                // The first notification may describe a change the initial
                // GetSlotStatus already saw, so only differences count.
                let first = !std::mem::replace(&mut self.synced, true);
                for (slot, known) in self.present.iter_mut().enumerate() {
                    let (now, changed) = Notification::slot_bits(bitmap, slot);
                    if now != *known {
                        push(slot, now);
                    } else if changed && !first {
                        // Same state but "changed": the card left and came
                        // back (or the reverse) between two notifications.
                        push(slot, !now);
                        push(slot, now);
                    }
                    *known = now;
                }
            }
            Notification::HardwareError { slot, code } => events.push(SlotEvent {
                slot: *slot,
                kind: SlotEventKind::HardwareError(*code),
            }),
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SlotEventKind::*;

    fn kinds(events: Vec<SlotEvent>) -> Vec<SlotEventKind> {
        events.into_iter().map(|e| e.kind).collect()
    }

    fn change(bits: u8) -> Notification {
        Notification::SlotChange(vec![bits])
    }

    #[test]
    fn tracks_arrival_and_removal() {
        let mut t = SlotTracker::new(vec![false]);
        assert_eq!(kinds(t.apply(&change(0b11))), [CardPresent]);
        assert_eq!(kinds(t.apply(&change(0b10))), [CardRemoved]);
        // Removed and back between two notifications.
        assert_eq!(kinds(t.apply(&change(0b11))), [CardPresent]);
        assert_eq!(kinds(t.apply(&change(0b11))), [CardRemoved, CardPresent]);
        let error = Notification::HardwareError { slot: 0, code: 1 };
        assert_eq!(kinds(t.apply(&error)), [HardwareError(1)]);
    }

    #[test]
    fn first_notification_only_reports_differences() {
        // The card arrived while the stream was being set up: the initial
        // status already saw it, so its notification adds nothing.
        let mut t = SlotTracker::new(vec![true]);
        assert!(t.apply(&change(0b11)).is_empty());
        assert_eq!(kinds(t.apply(&change(0b10))), [CardRemoved]);
    }
}
