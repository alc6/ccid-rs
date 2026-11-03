//! Vendor commands for ACS readers of the ACR1552U family, such as the
//! WalletMate II. Requires the `acs` feature.
//!
//! The commands travel in PC_to_RDR_Escape messages on the contactless
//! (PICC) interface. Byte layouts come from section 6 of the
//! [ACR1552U Series Reference Manual v1.09](https://www.acs.com.hk/en/download-manual/13474/REF-ACR1552U-Series-1.09.pdf).
//!
//! The manual says the reader saves the polling option and the UI behaviour.
//! Set them once when you set a reader up, not every time your program
//! starts.
//!
//! ```no_run
//! # async fn demo(reader: &mut ccid::Reader) -> Result<(), ccid::Error> {
//! use std::time::Duration;
//! use ccid::acs::{RfMode, RfOffInterval};
//!
//! let mut acs = reader.acs();
//! println!("firmware {}", acs.firmware_version().await?);
//! acs.beep(Duration::from_millis(100)).await?;
//!
//! // Poll less often: 1 s with the field off between polls.
//! let option = acs.polling_option().await?;
//! acs.set_polling_option(option.with_rf_off_interval(Some(RfOffInterval::Ms1000))).await?;
//!
//! // Switch the field off while a nearby payment terminal works, then back on.
//! acs.set_rf_mode(RfMode::Off).await?;
//! acs.set_rf_mode(RfMode::OnWithPolling).await?;
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use crate::{Error, Reader, Result};

impl Reader {
    /// ACS vendor commands. Requires the `acs` feature.
    pub fn acs(&mut self) -> Acs<'_> {
        Acs { reader: self }
    }
}

/// ACS vendor commands on a [`Reader`]. Get one with [`Reader::acs`].
#[derive(Debug)]
pub struct Acs<'r> {
    reader: &'r mut Reader,
}

impl Acs<'_> {
    /// Firmware version string, for example `ACR1552 W FW 5.00.14`.
    pub async fn firmware_version(&mut self) -> Result<String> {
        let data = self.command(0x00, 0x18, &[]).await?;
        Ok(text(&data))
    }

    /// Reader serial number.
    pub async fn serial_number(&mut self) -> Result<String> {
        let data = self.command(0x00, 0x33, &[]).await?;
        Ok(text(&data))
    }

    /// Sound the buzzer once. The duration is rounded to 10 ms; the longest
    /// beep is 2.55 s. A zero duration stops the buzzer.
    pub async fn beep(&mut self, duration: Duration) -> Result<()> {
        let units = (duration.as_millis() + 5) / 10;
        let units =
            u8::try_from(units).map_err(|_| Error::InvalidArgument("beep longer than 2.55 s"))?;
        self.command(0x00, 0x28, &[units]).await.map(drop)
    }

    /// Which LEDs are on.
    pub async fn leds(&mut self) -> Result<Leds> {
        Ok(Leds::from(self.command_byte(0x00, 0x29, &[]).await?))
    }

    /// Switch the LEDs on or off. Returns the state the reader reports back.
    ///
    /// The reader also drives the LEDs itself (see [`UiBehaviour`]), so it
    /// can change them again at the next card event.
    pub async fn set_leds(&mut self, leds: Leds) -> Result<Leds> {
        Ok(Leds::from(
            self.command_byte(0x00, 0x29, &[leds.into()]).await?,
        ))
    }

    /// How the reader uses its LEDs and buzzer on its own.
    pub async fn ui_behaviour(&mut self) -> Result<UiBehaviour> {
        Ok(UiBehaviour(self.command_byte(0x00, 0x21, &[]).await?))
    }

    /// Change how the reader uses its LEDs and buzzer on its own. The
    /// reader saves the setting. Returns the value it reports back.
    pub async fn set_ui_behaviour(&mut self, ui: UiBehaviour) -> Result<UiBehaviour> {
        Ok(UiBehaviour(self.command_byte(0x00, 0x21, &[ui.0]).await?))
    }

    /// How the reader polls for cards and builds their ATR.
    pub async fn polling_option(&mut self) -> Result<PollingOption> {
        Ok(PollingOption(self.command_byte(0x00, 0x23, &[]).await?))
    }

    /// Change how the reader polls for cards. The reader saves the setting.
    /// Returns the value it reports back.
    pub async fn set_polling_option(&mut self, option: PollingOption) -> Result<PollingOption> {
        Ok(PollingOption(
            self.command_byte(0x00, 0x23, &[option.0]).await?,
        ))
    }

    /// Switch the RF field and polling. This takes effect at once; use it to
    /// pause polling, for example while a nearby payment terminal works.
    pub async fn set_rf_mode(&mut self, mode: RfMode) -> Result<()> {
        let byte = match mode {
            RfMode::Off => 0x00,
            RfMode::OnWithPolling => 0x01,
            RfMode::OnWithoutPolling => 0x02,
        };
        self.command(0x00, 0x25, &[byte]).await.map(drop)
    }

    /// State of the RF field and of the card in it.
    pub async fn field_status(&mut self) -> Result<FieldStatus> {
        Ok(match self.command_byte(0x00, 0x25, &[]).await? {
            0x00 => FieldStatus::RfOff,
            0x01 => FieldStatus::NoCard,
            0x02 => FieldStatus::CardReady,
            0x03 => FieldStatus::CardActive,
            0xFF => FieldStatus::Error,
            other => FieldStatus::Unknown(other),
        })
    }

    /// Send one vendor command and return the data of the answer.
    async fn command(&mut self, p1: u8, p2: u8, data: &[u8]) -> Result<Vec<u8>> {
        let answer = self.reader.escape(0, &encode(p1, p2, data)).await?;
        decode(&answer)
    }

    /// Send one vendor command whose answer is a single byte.
    async fn command_byte(&mut self, p1: u8, p2: u8, data: &[u8]) -> Result<u8> {
        match self.command(p1, p2, data).await?.as_slice() {
            [byte] => Ok(*byte),
            other => Err(Error::Protocol(format!(
                "ACS command answered {} bytes, expected 1",
                other.len()
            ))),
        }
    }
}

/// Vendor command: `E0 00 P1 P2 Lc data`.
fn encode(p1: u8, p2: u8, data: &[u8]) -> Vec<u8> {
    let mut cmd = vec![0xE0, 0x00, p1, p2, data.len() as u8];
    cmd.extend_from_slice(data);
    cmd
}

/// Vendor answer: `E1 00 00 00 Le data`.
fn decode(answer: &[u8]) -> Result<Vec<u8>> {
    match answer {
        [0xE1, 0x00, 0x00, 0x00, len, data @ ..] if data.len() == usize::from(*len) => {
            Ok(data.to_vec())
        }
        _ => Err(Error::Protocol(format!(
            "unexpected ACS answer {answer:02X?}"
        ))),
    }
}

/// ASCII text from the reader, without trailing NUL bytes or spaces.
fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data)
        .trim_end_matches(['\0', ' '])
        .to_owned()
}

/// The two user LEDs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Leds {
    /// Blue LED on.
    pub blue: bool,
    /// Green LED on.
    pub green: bool,
}

impl From<u8> for Leds {
    fn from(byte: u8) -> Leds {
        Leds {
            blue: byte & 0x01 != 0,
            green: byte & 0x02 != 0,
        }
    }
}

impl From<Leds> for u8 {
    fn from(leds: Leds) -> u8 {
        u8::from(leds.blue) | u8::from(leds.green) << 1
    }
}

/// RF field mode, for [`Acs::set_rf_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RfMode {
    /// Field off: no card is detected and nothing is emitted.
    Off,
    /// Field on and the reader polls for cards (the normal mode).
    OnWithPolling,
    /// Field on, but the reader does not look for new cards.
    OnWithoutPolling,
}

/// State of the RF field, from [`Acs::field_status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldStatus {
    /// The field is off.
    RfOff,
    /// The field is on and no card is in it.
    NoCard,
    /// A card is in the field.
    CardReady,
    /// A card is in the field and activated.
    CardActive,
    /// The reader reports an error.
    Error,
    /// A value this crate does not know.
    Unknown(u8),
}

/// Time the field stays off between two polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RfOffInterval {
    /// 250 ms.
    Ms250,
    /// 500 ms.
    Ms500,
    /// 1 s.
    Ms1000,
    /// 2.5 s.
    Ms2500,
}

impl RfOffInterval {
    /// The interval as a duration.
    pub fn duration(self) -> Duration {
        Duration::from_millis(match self {
            RfOffInterval::Ms250 => 250,
            RfOffInterval::Ms500 => 500,
            RfOffInterval::Ms1000 => 1000,
            RfOffInterval::Ms2500 => 2500,
        })
    }
}

/// Polling and ATR settings of the reader.
///
/// Bits this crate does not name are kept as they are, so a value read
/// with [`Acs::polling_option`] can be changed and written back safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PollingOption(pub u8);

impl PollingOption {
    const POLLING: u8 = 0x01;
    const RF_OFF: u8 = 0x02;
    const MIFARE_TYPE_IN_ATR: u8 = 0x08;
    const INTERVAL_MASK: u8 = 0x30;
    const PART4_ATR_FOR_EMULATION: u8 = 0x80;

    /// The reader polls for cards on its own.
    pub fn polling(self) -> bool {
        self.0 & Self::POLLING != 0
    }

    /// With polling on or off.
    pub fn with_polling(self, on: bool) -> Self {
        self.set(Self::POLLING, on)
    }

    /// Time the field stays off between two polls, or `None` if the field
    /// stays on.
    pub fn rf_off_interval(self) -> Option<RfOffInterval> {
        if self.0 & Self::RF_OFF == 0 {
            return None;
        }
        Some(match (self.0 & Self::INTERVAL_MASK) >> 4 {
            0 => RfOffInterval::Ms250,
            1 => RfOffInterval::Ms500,
            2 => RfOffInterval::Ms1000,
            _ => RfOffInterval::Ms2500,
        })
    }

    /// With this RF off interval, or with the field always on (`None`).
    pub fn with_rf_off_interval(self, interval: Option<RfOffInterval>) -> Self {
        let Some(interval) = interval else {
            return self.set(Self::RF_OFF, false);
        };
        let bits = match interval {
            RfOffInterval::Ms250 => 0,
            RfOffInterval::Ms500 => 1,
            RfOffInterval::Ms1000 => 2,
            RfOffInterval::Ms2500 => 3,
        };
        let cleared = self.0 & !Self::INTERVAL_MASK;
        PollingOption(cleared | (bits << 4) | Self::RF_OFF)
    }

    /// The ATR of a MIFARE card names its exact type (Classic 1K, 4K, ...).
    pub fn mifare_type_in_atr(self) -> bool {
        self.0 & Self::MIFARE_TYPE_IN_ATR != 0
    }

    /// With or without the MIFARE type in the ATR.
    pub fn with_mifare_type_in_atr(self, on: bool) -> Self {
        self.set(Self::MIFARE_TYPE_IN_ATR, on)
    }

    /// Cards that emulate MIFARE on a smart card chip get an ISO 14443-4
    /// ATR instead of a MIFARE one.
    pub fn part4_atr_for_mifare_emulation(self) -> bool {
        self.0 & Self::PART4_ATR_FOR_EMULATION != 0
    }

    /// With or without the ISO 14443-4 ATR for MIFARE emulation.
    pub fn with_part4_atr_for_mifare_emulation(self, on: bool) -> Self {
        self.set(Self::PART4_ATR_FOR_EMULATION, on)
    }

    fn set(self, bit: u8, on: bool) -> Self {
        PollingOption(if on { self.0 | bit } else { self.0 & !bit })
    }
}

/// What the reader does with its LEDs and buzzer on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UiBehaviour(pub u8);

macro_rules! ui_flags {
    ($($bit:literal $get:ident $with:ident $doc:literal;)*) => {
        impl UiBehaviour {
            $(
                #[doc = $doc]
                pub fn $get(self) -> bool {
                    self.0 & (1 << $bit) != 0
                }

                #[doc = concat!("With or without this: ", $doc)]
                pub fn $with(self, on: bool) -> Self {
                    UiBehaviour(if on { self.0 | (1 << $bit) } else { self.0 & !(1 << $bit) })
                }
            )*
        }
    };
}

ui_flags! {
    0 blink_while_accessing with_blink_while_accessing "The LED blinks fast while the host talks to the card.";
    1 polling_led with_polling_led "The LED shows that the reader is polling.";
    2 activation_led with_activation_led "The LED shows that a card is activated.";
    3 beep_on_arrival with_beep_on_arrival "Short beep when a card arrives.";
    4 beep_on_removal with_beep_on_removal "Short beep when a card leaves.";
    5 beep_on_power_up with_beep_on_power_up "Short beep when the reader powers up.";
    6 blue_status_led with_blue_status_led "The blue LED shows status changes.";
    7 green_status_led with_green_status_led "The green LED shows status changes.";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_vendor_commands() {
        assert_eq!(encode(0x00, 0x18, &[]), [0xE0, 0x00, 0x00, 0x18, 0x00]);
        assert_eq!(
            encode(0x00, 0x28, &[0x0A]),
            [0xE0, 0x00, 0x00, 0x28, 0x01, 0x0A]
        );
        assert_eq!(decode(&[0xE1, 0, 0, 0, 2, 0x41, 0x42]).unwrap(), b"AB");
        // Wrong class byte, then a length that does not match.
        assert!(decode(&[0x90, 0x00]).is_err());
        assert!(decode(&[0xE1, 0, 0, 0, 3, 0x41]).is_err());
        assert_eq!(text(b"ACR1552 \0\0"), "ACR1552");
    }

    #[test]
    fn edits_settings_bit_by_bit() {
        // Factory polling option: polling, RF off interval of 250 ms, MIFARE
        // type in ATR, ISO 14443-4 ATR for emulation.
        let option = PollingOption(0x8B);
        assert!(option.polling() && option.mifare_type_in_atr());
        assert_eq!(option.rf_off_interval(), Some(RfOffInterval::Ms250));
        let slower = option.with_rf_off_interval(Some(RfOffInterval::Ms1000));
        assert_eq!(slower, PollingOption(0xAB));
        assert_eq!(slower.with_rf_off_interval(None).rf_off_interval(), None);
        // Unnamed bits (2 and 6) survive an edit.
        assert_eq!(PollingOption(0x44).with_polling(true), PollingOption(0x45));

        let ui = UiBehaviour(0x2F);
        assert!(ui.beep_on_arrival() && !ui.beep_on_removal());
        assert_eq!(ui.with_beep_on_arrival(false), UiBehaviour(0x27));
        assert_eq!(
            u8::from(Leds {
                blue: false,
                green: true
            }),
            0x02
        );
    }
}
