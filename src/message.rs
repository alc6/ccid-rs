//! CCID messages on the bulk pipes (USB CCID 1.1, sections 6.1 and 6.2).
//!
//! Every message has a 10-byte header: message type, `dwLength` (little
//! endian), `bSlot`, `bSeq` and three bytes whose meaning depends on the
//! message type. `dwLength` bytes of data follow.

use crate::{Error, Result};

/// Size of the header of every bulk message.
pub(crate) const HEADER_LEN: usize = 10;

/// Largest message this crate accepts from a reader: an extended APDU
/// response (65 536 data bytes + SW) plus the header. Anything larger is
/// treated as garbage.
const MAX_REPLY_LEN: usize = HEADER_LEN + 65_538;

// PC_to_RDR message types.
const ICC_POWER_ON: u8 = 0x62;
const ICC_POWER_OFF: u8 = 0x63;
const GET_SLOT_STATUS: u8 = 0x65;
const ESCAPE: u8 = 0x6B;
const XFR_BLOCK: u8 = 0x6F;
const ABORT: u8 = 0x72;

// RDR_to_PC message types.
pub(crate) const DATA_BLOCK: u8 = 0x80;
pub(crate) const SLOT_STATUS: u8 = 0x81;
pub(crate) const ESCAPE_REPLY: u8 = 0x83;

/// `wLevelParameter` values of PC_to_RDR_XfrBlock and `bChainParameter`
/// values of RDR_to_PC_DataBlock, for the extended APDU level.
pub(crate) mod chain {
    /// The APDU begins and ends in this message.
    pub const WHOLE: u16 = 0x0000;
    /// The APDU begins in this message and continues in the next one.
    pub const BEGIN: u16 = 0x0001;
    /// This message continues the APDU and ends it.
    pub const END: u16 = 0x0002;
    /// This message continues the APDU and more messages follow.
    pub const MIDDLE: u16 = 0x0003;
    /// Empty message: the other side should send the next part.
    pub const NEXT: u16 = 0x0010;
}

/// A command sent on the bulk OUT pipe.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Command<'a> {
    /// PC_to_RDR_IccPowerOn with automatic voltage selection.
    IccPowerOn,
    /// PC_to_RDR_IccPowerOff.
    IccPowerOff,
    /// PC_to_RDR_GetSlotStatus.
    GetSlotStatus,
    /// PC_to_RDR_XfrBlock. `level` is `wLevelParameter` (see [`chain`]).
    XfrBlock { level: u16, data: &'a [u8] },
    /// PC_to_RDR_Escape.
    Escape(&'a [u8]),
    /// PC_to_RDR_Abort, sent after the ABORT class request.
    Abort,
}

impl Command<'_> {
    /// Encode the command with the given slot and sequence number.
    pub(crate) fn encode(&self, slot: u8, seq: u8) -> Vec<u8> {
        let (kind, data, specific): (u8, &[u8], [u8; 3]) = match *self {
            // bPowerSelect 0 = automatic voltage selection.
            Command::IccPowerOn => (ICC_POWER_ON, &[], [0, 0, 0]),
            Command::IccPowerOff => (ICC_POWER_OFF, &[], [0, 0, 0]),
            Command::GetSlotStatus => (GET_SLOT_STATUS, &[], [0, 0, 0]),
            // bBWI 0: the reader uses the card's own block waiting time.
            Command::XfrBlock { level, data } => {
                let [lo, hi] = level.to_le_bytes();
                (XFR_BLOCK, data, [0, lo, hi])
            }
            Command::Escape(data) => (ESCAPE, data, [0, 0, 0]),
            Command::Abort => (ABORT, &[], [0, 0, 0]),
        };
        let mut msg = Vec::with_capacity(HEADER_LEN + data.len());
        msg.push(kind);
        msg.extend_from_slice(&(data.len() as u32).to_le_bytes());
        msg.extend_from_slice(&[slot, seq]);
        msg.extend_from_slice(&specific);
        msg.extend_from_slice(data);
        msg
    }

    /// Message type of the reply the reader sends to this command.
    pub(crate) fn reply_kind(&self) -> u8 {
        match self {
            Command::IccPowerOn | Command::XfrBlock { .. } => DATA_BLOCK,
            Command::IccPowerOff | Command::GetSlotStatus | Command::Abort => SLOT_STATUS,
            Command::Escape(_) => ESCAPE_REPLY,
        }
    }

    /// Name for log messages.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Command::IccPowerOn => "IccPowerOn",
            Command::IccPowerOff => "IccPowerOff",
            Command::GetSlotStatus => "GetSlotStatus",
            Command::XfrBlock { .. } => "XfrBlock",
            Command::Escape(_) => "Escape",
            Command::Abort => "Abort",
        }
    }
}

/// Presence and state of the card in a slot (`bmICCStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IccStatus {
    /// A card is present and powered.
    Active,
    /// A card is present but not powered.
    Inactive,
    /// No card is present.
    Absent,
}

/// Outcome of a command (`bmCommandStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandStatus {
    /// The command succeeded.
    Processed,
    /// The command failed; `bError` holds the slot error.
    Failed,
    /// The reader needs more time; the real reply follows.
    TimeExtension,
}

/// A reply received on the bulk IN pipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reply {
    pub kind: u8,
    pub slot: u8,
    pub seq: u8,
    pub status: u8,
    pub error: u8,
    /// `bChainParameter` (DataBlock), `bClockStatus` (SlotStatus) or RFU.
    pub param: u8,
    pub data: Vec<u8>,
}

impl Reply {
    /// Decode exactly one complete message.
    pub(crate) fn decode(msg: &[u8]) -> Result<Reply> {
        let Some(len) = message_len(msg)? else {
            return Err(Error::Protocol(format!(
                "reply of {} bytes is shorter than the header",
                msg.len()
            )));
        };
        if msg.len() != len {
            return Err(Error::Protocol(format!(
                "reply is {} bytes but its header announces {len}",
                msg.len()
            )));
        }
        Ok(Reply {
            kind: msg[0],
            slot: msg[5],
            seq: msg[6],
            status: msg[7],
            error: msg[8],
            param: msg[9],
            data: msg[HEADER_LEN..].to_vec(),
        })
    }

    /// Card presence and state reported with this reply.
    pub(crate) fn icc_status(&self) -> Result<IccStatus> {
        match self.status & 0x03 {
            0 => Ok(IccStatus::Active),
            1 => Ok(IccStatus::Inactive),
            2 => Ok(IccStatus::Absent),
            _ => Err(Error::Protocol(format!(
                "reserved card status in bStatus 0x{:02x}",
                self.status
            ))),
        }
    }

    /// Outcome of the command.
    pub(crate) fn command_status(&self) -> Result<CommandStatus> {
        match self.status >> 6 {
            0 => Ok(CommandStatus::Processed),
            1 => Ok(CommandStatus::Failed),
            2 => Ok(CommandStatus::TimeExtension),
            _ => Err(Error::Protocol(format!(
                "reserved command status in bStatus 0x{:02x}",
                self.status
            ))),
        }
    }
}

/// Whether `kind` is a message type the reader sends on bulk IN.
pub(crate) fn is_reply_kind(kind: u8) -> bool {
    matches!(kind, 0x80..=0x84)
}

/// Total length of the message that starts at `buf[0]`, or `None` until the
/// header is complete.
pub(crate) fn message_len(buf: &[u8]) -> Result<Option<usize>> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let data_len = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
    let len = HEADER_LEN.saturating_add(data_len);
    if len > MAX_REPLY_LEN {
        return Err(Error::Protocol(format!(
            "reply announces {data_len} data bytes"
        )));
    }
    Ok(Some(len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_commands() {
        assert_eq!(
            Command::IccPowerOn.encode(0, 7),
            [0x62, 0, 0, 0, 0, 0, 7, 0, 0, 0]
        );
        assert_eq!(
            Command::GetSlotStatus.encode(1, 0xFF),
            [0x65, 0, 0, 0, 0, 1, 0xFF, 0, 0, 0]
        );
        assert_eq!(
            Command::XfrBlock {
                level: chain::NEXT,
                data: &[0xFF, 0xCA, 0x00, 0x00, 0x00],
            }
            .encode(0, 2),
            [
                0x6F, 5, 0, 0, 0, 0, 2, 0, 0x10, 0x00, 0xFF, 0xCA, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(
            Command::Escape(&[0xE0, 0x00]).encode(0, 3),
            [0x6B, 2, 0, 0, 0, 0, 3, 0, 0, 0, 0xE0, 0x00]
        );
        assert_eq!(Command::Abort.encode(0, 4)[0], 0x72);
        assert_eq!(Command::IccPowerOff.encode(0, 5)[0], 0x63);
    }

    #[test]
    fn decodes_replies() {
        // DataBlock: card active, success, chain parameter 0x01, 2 bytes.
        let reply = Reply::decode(&[0x80, 2, 0, 0, 0, 0, 9, 0x00, 0x00, 0x01, 0x90, 0x00]).unwrap();
        assert_eq!((reply.kind, reply.slot, reply.seq), (DATA_BLOCK, 0, 9));
        assert_eq!(reply.param, 0x01);
        assert_eq!(reply.data, [0x90, 0x00]);
        assert_eq!(reply.icc_status().unwrap(), IccStatus::Active);
        assert_eq!(reply.command_status().unwrap(), CommandStatus::Processed);

        // SlotStatus: no card, command failed, ICC_MUTE.
        let reply = Reply::decode(&[0x81, 0, 0, 0, 0, 0, 1, 0x42, 0xFE, 0x00]).unwrap();
        assert_eq!(reply.icc_status().unwrap(), IccStatus::Absent);
        assert_eq!(reply.command_status().unwrap(), CommandStatus::Failed);

        // Time extension request.
        let reply = Reply::decode(&[0x80, 0, 0, 0, 0, 0, 1, 0x80, 0x01, 0x00]).unwrap();
        assert_eq!(
            reply.command_status().unwrap(),
            CommandStatus::TimeExtension
        );
    }

    #[test]
    fn rejects_malformed_replies() {
        // Shorter than the header.
        assert!(Reply::decode(&[0x80, 0, 0, 0]).is_err());
        // dwLength says 3 bytes but only 2 follow.
        assert!(Reply::decode(&[0x80, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0x90, 0x00]).is_err());
        // Absurd dwLength.
        assert!(message_len(&[0x80, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0]).is_err());
        // Reserved status bits.
        let reply = Reply::decode(&[0x81, 0, 0, 0, 0, 0, 0, 0xC3, 0, 0]).unwrap();
        assert!(reply.icc_status().is_err());
        assert!(reply.command_status().is_err());
    }
}
