//! The CCID command/reply exchange on the bulk pipes.

use std::time::{Duration, Instant};

use crate::atr::Atr;
use crate::descriptor::{CcidDescriptor, ExchangeLevel};
use crate::error::SlotError;
use crate::message::{self, Command, CommandStatus, IccStatus, Reply, chain};
use crate::transport::Transport;
use crate::{Error, Result};

/// Default time to wait for each reply.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Time allowed to resynchronise after a timeout.
const RECOVERY_TIMEOUT: Duration = Duration::from_millis(500);

/// Runs CCID commands on one interface, one at a time.
pub(crate) struct Protocol {
    transport: Box<dyn Transport>,
    descriptor: CcidDescriptor,
    seq: u8,
    pub(crate) timeout: Duration,
}

impl Protocol {
    pub(crate) fn new(transport: Box<dyn Transport>, descriptor: CcidDescriptor) -> Protocol {
        Protocol {
            transport,
            descriptor,
            seq: 0,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub(crate) fn descriptor(&self) -> &CcidDescriptor {
        &self.descriptor
    }

    pub(crate) async fn slot_status(&mut self, slot: u8) -> Result<IccStatus> {
        let reply = self.run(slot, Command::GetSlotStatus).await?;
        reply.icc_status()
    }

    pub(crate) async fn power_on(&mut self, slot: u8) -> Result<Atr> {
        let reply = self.run(slot, Command::IccPowerOn).await?;
        Ok(Atr::new(reply.data))
    }

    pub(crate) async fn power_off(&mut self, slot: u8) -> Result<()> {
        self.run(slot, Command::IccPowerOff).await.map(drop)
    }

    pub(crate) async fn escape(&mut self, slot: u8, data: &[u8]) -> Result<Vec<u8>> {
        self.check_len(data.len())?;
        Ok(self.run(slot, Command::Escape(data)).await?.data)
    }

    /// Send a command APDU and return the raw response APDU.
    ///
    /// At the extended APDU level, APDUs that do not fit in one message are
    /// split with `wLevelParameter`, and chained responses are joined.
    pub(crate) async fn transmit(&mut self, slot: u8, apdu: &[u8]) -> Result<Vec<u8>> {
        let extended = self.descriptor.exchange_level() == Some(ExchangeLevel::ExtendedApdu);
        let max_data = self.max_data_len();
        let mut reply = if apdu.len() <= max_data {
            self.xfr(slot, chain::WHOLE, apdu).await?
        } else if extended {
            self.send_chained(slot, apdu, max_data).await?
        } else {
            return Err(Error::InvalidArgument(
                "APDU longer than the reader's maximum message",
            ));
        };
        if !extended {
            return Ok(reply.data);
        }
        let mut response = std::mem::take(&mut reply.data);
        let mut param = u16::from(reply.param);
        while param == chain::BEGIN || param == chain::MIDDLE {
            let next = self.xfr(slot, chain::NEXT, &[]).await?;
            response.extend_from_slice(&next.data);
            param = u16::from(next.param);
        }
        if param != chain::WHOLE && param != chain::END {
            return Err(Error::Protocol(format!(
                "unexpected bChainParameter 0x{param:02x} in a response"
            )));
        }
        Ok(response)
    }

    /// Send an APDU in several XfrBlock messages and return the reply to the
    /// last one.
    async fn send_chained(&mut self, slot: u8, apdu: &[u8], max_data: usize) -> Result<Reply> {
        let parts: Vec<&[u8]> = apdu.chunks(max_data).collect();
        let last = parts.len() - 1;
        for (i, part) in parts.iter().enumerate() {
            let level = match i {
                0 => chain::BEGIN,
                i if i == last => chain::END,
                _ => chain::MIDDLE,
            };
            let reply = self.xfr(slot, level, part).await?;
            if i == last {
                return Ok(reply);
            }
            if u16::from(reply.param) != chain::NEXT {
                return Err(Error::Protocol(format!(
                    "reader answered bChainParameter 0x{:02x} to part {} of a chained APDU",
                    reply.param,
                    i + 1
                )));
            }
        }
        unreachable!("an APDU longer than max_data has at least two parts")
    }

    async fn xfr(&mut self, slot: u8, level: u16, data: &[u8]) -> Result<Reply> {
        self.run(slot, Command::XfrBlock { level, data }).await
    }

    /// Largest payload of one message.
    fn max_data_len(&self) -> usize {
        (self.descriptor.max_message_length as usize).saturating_sub(message::HEADER_LEN)
    }

    fn check_len(&self, len: usize) -> Result<()> {
        if len > self.max_data_len() {
            return Err(Error::InvalidArgument(
                "data longer than the reader's maximum message",
            ));
        }
        Ok(())
    }

    /// Run one command and turn a failed status into an error.
    async fn run(&mut self, slot: u8, cmd: Command<'_>) -> Result<Reply> {
        let reply = self.exchange(slot, cmd).await?;
        if reply.command_status()? == CommandStatus::Failed {
            let absent = reply.icc_status()? == IccStatus::Absent;
            return Err(match cmd {
                Command::IccPowerOn if absent => Error::NoCard,
                Command::XfrBlock { .. } if absent => Error::CardRemoved,
                _ => Error::Slot(SlotError::from(reply.error)),
            });
        }
        Ok(reply)
    }

    /// Send one command and wait for its reply.
    ///
    /// Replies with an older sequence number (left over from a command that
    /// timed out) are dropped. Time extension requests restart the timeout.
    async fn exchange(&mut self, slot: u8, cmd: Command<'_>) -> Result<Reply> {
        if slot > self.descriptor.max_slot_index {
            return Err(Error::InvalidArgument("slot number out of range"));
        }
        let seq = self.next_seq();
        let msg = cmd.encode(slot, seq);
        log::trace!("-> {} slot {slot} seq {seq}: {}", cmd.name(), Hex(&msg));
        self.transport.write(&msg, self.timeout).await?;

        let mut deadline = Instant::now() + self.timeout;
        loop {
            let reply = match self.read_reply(deadline).await {
                Err(Error::Timeout) => {
                    self.recover(slot).await;
                    return Err(Error::Timeout);
                }
                other => other?,
            };
            if reply.seq != seq {
                log::debug!(
                    "dropping reply with bSeq {} while waiting for {seq}",
                    reply.seq
                );
                continue;
            }
            if reply.kind != cmd.reply_kind() || reply.slot != slot {
                return Err(Error::Protocol(format!(
                    "{} got reply type 0x{:02x} for slot {}",
                    cmd.name(),
                    reply.kind,
                    reply.slot
                )));
            }
            if reply.command_status()? == CommandStatus::TimeExtension {
                log::trace!("time extension requested (multiplier {})", reply.error);
                deadline = Instant::now() + self.timeout;
                continue;
            }
            return Ok(reply);
        }
    }

    /// Read bulk IN transfers until one whole message is in.
    async fn read_reply(&mut self, deadline: Instant) -> Result<Reply> {
        let mut buf = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let chunk = self.transport.read(left).await?;
            if buf.is_empty() {
                if chunk.is_empty() {
                    continue; // zero-length packet ending the previous message
                }
                if !message::is_reply_kind(chunk[0]) {
                    log::warn!("dropping {} bytes that do not start a reply", chunk.len());
                    continue;
                }
            }
            buf.extend_from_slice(&chunk);
            if let Some(len) = message::message_len(&buf)? {
                if buf.len() >= len {
                    if buf.len() > len {
                        log::warn!("dropping {} bytes after a reply", buf.len() - len);
                        buf.truncate(len);
                    }
                    log::trace!("<- {}", Hex(&buf));
                    return Reply::decode(&buf);
                }
            }
        }
    }

    /// After a timeout, ask the reader to abort the slot's current command
    /// and drain its reply, so the next command starts clean. Best effort:
    /// errors are only logged.
    async fn recover(&mut self, slot: u8) {
        let seq = self.next_seq();
        if let Err(e) = self.transport.abort(slot, seq).await {
            log::debug!("ABORT request failed: {e}");
        }
        let msg = Command::Abort.encode(slot, seq);
        if let Err(e) = self.transport.write(&msg, RECOVERY_TIMEOUT).await {
            log::debug!("PC_to_RDR_Abort failed: {e}");
            return;
        }
        let deadline = Instant::now() + RECOVERY_TIMEOUT;
        while let Ok(reply) = self.read_reply(deadline).await {
            if reply.seq == seq {
                break;
            }
        }
    }

    fn next_seq(&mut self) -> u8 {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        seq
    }
}

/// Hex formatting for log messages.
struct Hex<'a>(&'a [u8]);

impl std::fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use futures_lite::future::block_on;

    use super::*;
    use crate::transport::fake::FakeTransport;

    /// Descriptor of a one-slot, extended APDU level reader whose messages
    /// carry at most `max_data` bytes.
    pub(crate) fn descriptor(level: u32, max_data: u32) -> CcidDescriptor {
        let mut raw = [0u8; 54];
        raw[0] = 54;
        raw[1] = 0x21;
        raw[40..44].copy_from_slice(&level.to_le_bytes());
        raw[44..48].copy_from_slice(&(max_data + 10).to_le_bytes());
        CcidDescriptor::parse(&raw).unwrap()
    }

    pub(crate) fn protocol(level: u32, max_data: u32) -> (Protocol, FakeTransport) {
        let fake = FakeTransport::default();
        let protocol = Protocol::new(Box::new(fake.clone()), descriptor(level, max_data));
        (protocol, fake)
    }

    pub(crate) const EXTENDED: u32 = 0x0004_0000;
    pub(crate) const SHORT: u32 = 0x0002_0000;

    /// Build a reply message.
    pub(crate) fn reply(
        kind: u8,
        seq: u8,
        status: u8,
        error: u8,
        param: u8,
        data: &[u8],
    ) -> Vec<u8> {
        let mut msg = vec![kind];
        msg.extend_from_slice(&(data.len() as u32).to_le_bytes());
        msg.extend_from_slice(&[0, seq, status, error, param]);
        msg.extend_from_slice(data);
        msg
    }

    #[test]
    fn power_on_returns_atr_and_counts_seq() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        fake.push_read(&reply(0x81, 0, 0x00, 0, 0, &[]));
        fake.push_read(&reply(0x80, 1, 0x00, 0, 0, &[0x3B, 0x80, 0x80, 0x01, 0x01]));
        assert_eq!(block_on(p.slot_status(0)).unwrap(), IccStatus::Active);
        let atr = block_on(p.power_on(0)).unwrap();
        assert_eq!(atr.as_bytes(), [0x3B, 0x80, 0x80, 0x01, 0x01]);
        let writes = fake.writes();
        assert_eq!((writes[0][0], writes[0][6]), (0x65, 0));
        assert_eq!((writes[1][0], writes[1][6]), (0x62, 1));
    }

    #[test]
    fn reassembles_split_replies_and_skips_noise() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        let data: Vec<u8> = (0..100).collect();
        let msg = reply(0x80, 0, 0x00, 0, 0, &[&data[..], &[0x90, 0x00]].concat());
        fake.push_read(&[]); // stray zero-length packet
        fake.push_read(&reply(0x80, 0xFF, 0x00, 0, 0, &[0x6F, 0x00])); // stale bSeq
        fake.push_read(&msg[..64]);
        fake.push_read(&msg[64..]);
        let response = block_on(p.transmit(0, &[0x00, 0xB0, 0x00, 0x00, 0x64])).unwrap();
        assert_eq!(response, [&data[..], &[0x90, 0x00]].concat());
    }

    #[test]
    fn waits_through_time_extensions() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        fake.push_read(&reply(0x80, 0, 0x80, 0x01, 0, &[]));
        fake.push_read(&reply(0x80, 0, 0x80, 0x01, 0, &[]));
        fake.push_read(&reply(0x80, 0, 0x00, 0, 0, &[0x90, 0x00]));
        assert_eq!(
            block_on(p.transmit(0, &[0x00, 0x84, 0x00, 0x00, 0x08])).unwrap(),
            [0x90, 0x00]
        );
    }

    #[test]
    fn maps_slot_errors() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        // No card on power on.
        fake.push_read(&reply(0x80, 0, 0x42, 0xFE, 0, &[]));
        assert!(matches!(block_on(p.power_on(0)), Err(Error::NoCard)));
        // Card gone during an APDU.
        fake.push_read(&reply(0x80, 1, 0x42, 0xFE, 0, &[]));
        assert!(matches!(
            block_on(p.transmit(0, &[0; 5])),
            Err(Error::CardRemoved)
        ));
        // Card present but mute.
        fake.push_read(&reply(0x80, 2, 0x40, 0xFE, 0, &[]));
        assert!(matches!(
            block_on(p.transmit(0, &[0; 5])),
            Err(Error::Slot(SlotError::IccMute))
        ));
        // Wrong reply type for the sequence number.
        fake.push_read(&reply(0x81, 3, 0x00, 0, 0, &[]));
        assert!(matches!(
            block_on(p.transmit(0, &[0; 5])),
            Err(Error::Protocol(_))
        ));
        // Slot out of range.
        assert!(matches!(
            block_on(p.power_on(1)),
            Err(Error::InvalidArgument(_))
        ));
        // Reader unplugged while waiting for the reply: no abort attempt.
        fake.push_read_error(Error::Disconnected);
        assert!(matches!(block_on(p.power_on(0)), Err(Error::Disconnected)));
        assert!(fake.aborts().is_empty());
    }

    #[test]
    fn timeout_aborts_and_next_command_drops_the_late_reply() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        // The fake has nothing to read: the command times out, then the
        // abort's reply times out too.
        assert!(matches!(block_on(p.power_on(0)), Err(Error::Timeout)));
        assert_eq!(fake.aborts(), [(0, 1)]);
        let writes = fake.writes();
        assert_eq!((writes[1][0], writes[1][6]), (0x72, 1));
        // The late reply to power on (seq 0) arrives before the next reply.
        fake.push_read(&reply(0x80, 0, 0x00, 0, 0, &[0x3B, 0x00]));
        fake.push_read(&reply(0x81, 2, 0x00, 0, 0, &[]));
        assert_eq!(block_on(p.slot_status(0)).unwrap(), IccStatus::Active);
    }

    #[test]
    fn chains_extended_apdus_both_ways() {
        let (mut p, fake) = protocol(EXTENDED, 4);
        let apdu = [0x00, 0xD6, 0x00, 0x00, 0x03, 0xAA, 0xBB, 0xCC, 0x00];
        // Command in 3 parts: the reader asks for more after parts 1 and 2.
        fake.push_read(&reply(0x80, 0, 0x00, 0, 0x10, &[]));
        fake.push_read(&reply(0x80, 1, 0x00, 0, 0x10, &[]));
        // Response in 2 parts.
        fake.push_read(&reply(0x80, 2, 0x00, 0, 0x01, &[0x01, 0x02, 0x03, 0x04]));
        fake.push_read(&reply(0x80, 3, 0x00, 0, 0x02, &[0x05, 0x90, 0x00]));
        let response = block_on(p.transmit(0, &apdu)).unwrap();
        assert_eq!(response, [0x01, 0x02, 0x03, 0x04, 0x05, 0x90, 0x00]);
        // (wLevelParameter, data) of each XfrBlock sent.
        let sent: Vec<(u16, Vec<u8>)> = fake
            .writes()
            .iter()
            .map(|w| (u16::from_le_bytes([w[8], w[9]]), w[10..].to_vec()))
            .collect();
        assert_eq!(
            sent,
            [
                (0x01, apdu[0..4].to_vec()),
                (0x03, apdu[4..8].to_vec()),
                (0x02, apdu[8..].to_vec()),
                (0x10, vec![]),
            ]
        );
    }

    #[test]
    fn short_level_rejects_long_apdus() {
        let (mut p, _fake) = protocol(SHORT, 4);
        assert!(matches!(
            block_on(p.transmit(0, &[0; 5])),
            Err(Error::InvalidArgument(_))
        ));
    }
}
