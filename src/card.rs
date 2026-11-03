//! A powered card and the standard helpers that talk to it.

use crate::Result;
use crate::apdu::{self, Response};
use crate::atr::Atr;
use crate::protocol::Protocol;

/// Most GET RESPONSE rounds `select_aid` follows before giving up.
const MAX_GET_RESPONSE: usize = 64;

/// A powered card in one slot of a [`Reader`](crate::Reader).
///
/// Get one with [`Reader::connect`](crate::Reader::connect). It borrows the
/// reader, so only one exchange runs at a time. Dropping it leaves the card
/// powered; call [`disconnect`](Self::disconnect) to power it off.
pub struct Card<'r> {
    protocol: &'r mut Protocol,
    slot: u8,
    atr: Atr,
}

impl<'r> Card<'r> {
    pub(crate) fn new(protocol: &'r mut Protocol, slot: u8, atr: Atr) -> Card<'r> {
        Card {
            protocol,
            slot,
            atr,
        }
    }

    /// ATR returned when the card was powered on.
    pub fn atr(&self) -> &Atr {
        &self.atr
    }

    /// Slot the card is in.
    pub fn slot(&self) -> u8 {
        self.slot
    }

    /// Send a command APDU and return the response APDU.
    ///
    /// Status words are returned as they are: `61 XX` and `6C XX` are not
    /// handled here.
    pub async fn transmit(&mut self, apdu: &[u8]) -> Result<Response> {
        let raw = self.protocol.transmit(self.slot, apdu).await?;
        Response::from_bytes(&raw)
    }

    /// Read the card UID with the PC/SC Part 3 GET DATA pseudo-APDU
    /// (`FF CA 00 00 00`).
    ///
    /// Returns [`Error::Status`](crate::Error::Status) if the reader or card
    /// does not answer `90 00`.
    pub async fn get_uid(&mut self) -> Result<Vec<u8>> {
        self.transmit(&[0xFF, 0xCA, 0x00, 0x00, 0x00])
            .await?
            .into_data()
    }

    /// Select an application by AID (`00 A4 04 00 Lc AID 00`).
    ///
    /// The status word is returned as it is, so `6A 82` (not found) is a
    /// normal result. `61 XX` is followed with GET RESPONSE and `6C XX` is
    /// retried with the right Le (ISO/IEC 7816-4, section 5.3.4), so the
    /// returned data is complete.
    pub async fn select_aid(&mut self, aid: &[u8]) -> Result<Response> {
        self.transmit_with_get_response([0x00, 0xA4, 0x04, 0x00], aid, 256)
            .await
    }

    /// Power the card off.
    pub async fn disconnect(self) -> Result<()> {
        self.protocol.power_off(self.slot).await
    }

    async fn transmit_with_get_response(
        &mut self,
        header: [u8; 4],
        data: &[u8],
        le: usize,
    ) -> Result<Response> {
        let mut response = self
            .transmit(&apdu::command(header, data, Some(le))?)
            .await?;
        if response.sw.sw1() == 0x6C {
            let le = le_from_sw2(response.sw.sw2());
            response = self
                .transmit(&apdu::command(header, data, Some(le))?)
                .await?;
        }
        let mut collected = Vec::new();
        for _ in 0..MAX_GET_RESPONSE {
            if response.sw.sw1() != 0x61 {
                break;
            }
            collected.append(&mut response.data);
            let le = le_from_sw2(response.sw.sw2());
            let get_response = [header[0] & 0x03, 0xC0, 0x00, 0x00];
            response = self
                .transmit(&apdu::command(get_response, &[], Some(le))?)
                .await?;
        }
        collected.append(&mut response.data);
        Ok(Response {
            data: collected,
            sw: response.sw,
        })
    }
}

impl std::fmt::Debug for Card<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("slot", &self.slot)
            .field("atr", &self.atr)
            .finish_non_exhaustive()
    }
}

/// `XX` of `61 XX` or `6C XX` as a length: 0 means 256.
fn le_from_sw2(sw2: u8) -> usize {
    if sw2 == 0 { 256 } else { usize::from(sw2) }
}

#[cfg(test)]
mod tests {
    use futures_lite::future::block_on;

    use super::*;
    use crate::Error;
    use crate::apdu::StatusWord;
    use crate::protocol::tests::{EXTENDED, protocol, reply};

    fn card(protocol: &mut Protocol) -> Card<'_> {
        Card::new(protocol, 0, Atr::new(vec![0x3B, 0x80, 0x80, 0x01, 0x01]))
    }

    #[test]
    fn get_uid_sends_get_data() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        fake.push_read(&reply(
            0x80,
            0,
            0,
            0,
            0,
            &[0x01, 0x02, 0x03, 0x04, 0x90, 0x00],
        ));
        fake.push_read(&reply(0x80, 1, 0, 0, 0, &[0x6A, 0x81]));
        let mut card = card(&mut p);
        assert_eq!(block_on(card.get_uid()).unwrap(), [0x01, 0x02, 0x03, 0x04]);
        assert!(matches!(
            block_on(card.get_uid()),
            Err(Error::Status(StatusWord(0x6A81)))
        ));
        assert_eq!(fake.writes()[0][10..], [0xFF, 0xCA, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn select_aid_follows_61xx_and_6cxx() {
        let (mut p, fake) = protocol(EXTENDED, 506);
        fake.push_read(&reply(0x80, 0, 0, 0, 0, &[0x6C, 0x04]));
        fake.push_read(&reply(0x80, 1, 0, 0, 0, &[0x6F, 0x01, 0x61, 0x03]));
        fake.push_read(&reply(0x80, 2, 0, 0, 0, &[0xAA, 0xBB, 0xCC, 0x90, 0x00]));
        let aid = [0xA0, 0x00, 0x00, 0x00, 0x00];
        let response = block_on(card(&mut p).select_aid(&aid)).unwrap();
        assert_eq!(response.data, [0x6F, 0x01, 0xAA, 0xBB, 0xCC]);
        assert!(response.sw.is_success());
        let apdus: Vec<Vec<u8>> = fake.writes().iter().map(|w| w[10..].to_vec()).collect();
        assert_eq!(
            apdus[0],
            [
                0x00, 0xA4, 0x04, 0x00, 0x05, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(
            apdus[1],
            [
                0x00, 0xA4, 0x04, 0x00, 0x05, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x04
            ]
        );
        assert_eq!(apdus[2], [0x00, 0xC0, 0x00, 0x00, 0x03]);
    }
}
