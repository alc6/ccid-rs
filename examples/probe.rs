//! Probe CCID readers.
//!
//! ```text
//! cargo run --example probe                     # list CCID interfaces
//! cargo run --example probe -- open             # claim the first interface
//! cargo run --example probe -- read             # power on the card, print ATR and UID
//! cargo run --example probe -- read --taps 3 --aid ppse
//! ```
//!
//! Options:
//!
//! - `--reader VID:PID` and `--interface N` select the interface (default:
//!   the first CCID interface found).
//! - `--taps N` (read): read N cards in a row. The probe waits for each card
//!   to arrive and leave by asking the slot status every 20 ms.
//! - `--aid HEX|ppse` (read): also SELECT this AID and print the status word.

use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use ccid::IccStatus;

/// `2PAY.SYS.DDF01`, the contactless payment directory.
const PPSE: &[u8] = b"2PAY.SYS.DDF01";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, rest) = match args.split_first() {
        Some((command, rest)) => (command.as_str(), rest),
        None => ("list", &[][..]),
    };
    let options = match Options::parse(rest) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let result = match command {
        "list" => list(),
        "open" => open(&options),
        "read" => read(&options),
        other => {
            eprintln!("unknown command {other:?}; expected `list`, `open` or `read`");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn list() -> Result<(), ccid::Error> {
    let readers = ccid::list_readers()?;
    if readers.is_empty() {
        println!("no CCID interface found");
    }
    for info in readers {
        println!(
            "{:04x}:{:04x} interface {}  \"{}\"  ({} / {})",
            info.vendor_id(),
            info.product_id(),
            info.interface_number(),
            info.interface_name().unwrap_or("?"),
            info.manufacturer().unwrap_or("?"),
            info.product().unwrap_or("?"),
        );
    }
    Ok(())
}

/// Find and open the interface selected by the options.
///
/// If another driver holds the interface, retry for up to 2 seconds: on
/// macOS that is the window between stopping usbsmartcardreaderd and
/// launchd starting it again.
fn open_reader(options: &Options) -> Result<ccid::blocking::Reader, ccid::Error> {
    let info = ccid::list_readers()?
        .into_iter()
        .find(|r| options.matches(r))
        .ok_or(ccid::Error::NotFound)?;
    let start = Instant::now();
    let reader = loop {
        match info.open() {
            Err(ccid::Error::Busy) if start.elapsed() < Duration::from_secs(2) => {
                sleep(Duration::from_millis(10));
            }
            other => break other?,
        }
    };
    println!(
        "reader   {:04x}:{:04x} interface {} \"{}\"",
        info.vendor_id(),
        info.product_id(),
        info.interface_number(),
        info.interface_name().unwrap_or("?"),
    );
    Ok(reader.into())
}

fn open(options: &Options) -> Result<(), ccid::Error> {
    let reader = open_reader(options)?;
    let level = reader
        .descriptor()
        .exchange_level()
        .map_or("undefined".to_owned(), |l| l.to_string());
    println!("claimed: {reader:?}");
    println!("exchange level: {level}");
    Ok(())
}

fn read(options: &Options) -> Result<(), ccid::Error> {
    let mut reader = open_reader(options)?;
    for tap in 1..=options.taps {
        if options.taps > 1 {
            println!("\n[{tap}/{}] put a card on the reader", options.taps);
            wait_for_card(&mut reader, true)?;
        }
        read_card(&mut reader, options)?;
        if options.taps > 1 {
            println!("remove the card");
            wait_for_card(&mut reader, false)?;
        }
    }
    Ok(())
}

/// Power on the card in slot 0 and print what it says.
fn read_card(reader: &mut ccid::blocking::Reader, options: &Options) -> Result<(), ccid::Error> {
    let start = Instant::now();
    let mut card = reader.connect(0)?;
    let power_on = start.elapsed();
    let atr = card.atr().clone();
    println!("ATR      {atr}");
    println!("type     {}", atr.card_type());

    let start = Instant::now();
    let uid = card.get_uid();
    let uid_time = start.elapsed();
    match uid {
        Ok(uid) => println!("UID      {} ({} bytes)", hex(&uid), uid.len()),
        Err(e) => println!("UID      error: {e}"),
    }
    println!("latency  power on {}, UID {}", ms(power_on), ms(uid_time));

    if let Some(aid) = &options.aid {
        let start = Instant::now();
        let response = card.select_aid(aid)?;
        println!(
            "SELECT   {} -> SW {} with {} data bytes ({})",
            hex(aid),
            response.sw,
            response.data.len(),
            ms(start.elapsed())
        );
        if !response.data.is_empty() {
            println!("data     {}", hex(&response.data));
        }
    }
    Ok(())
}

/// Ask the slot status until a card is present (or absent).
fn wait_for_card(reader: &mut ccid::blocking::Reader, present: bool) -> Result<(), ccid::Error> {
    loop {
        let is_present = reader.slot_status(0)? != IccStatus::Absent;
        if is_present == present {
            return Ok(());
        }
        sleep(Duration::from_millis(20));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1e3)
}

/// Command-line options.
struct Options {
    ids: Option<(u16, u16)>,
    interface: Option<u8>,
    taps: u32,
    aid: Option<Vec<u8>>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Options, String> {
        let mut options = Options {
            ids: None,
            interface: None,
            taps: 1,
            aid: None,
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or(format!("{arg} needs a value"));
            match arg.as_str() {
                "--reader" => {
                    let v = value()?;
                    let (vid, pid) = v.split_once(':').ok_or("--reader needs VID:PID")?;
                    let hex = |s| u16::from_str_radix(s, 16).map_err(|_| "VID and PID are hex");
                    options.ids = Some((hex(vid)?, hex(pid)?));
                }
                "--interface" => {
                    options.interface = Some(value()?.parse().map_err(|_| "bad interface")?);
                }
                "--taps" => options.taps = value()?.parse().map_err(|_| "bad tap count")?,
                "--aid" => {
                    let v = value()?;
                    options.aid = Some(if v.eq_ignore_ascii_case("ppse") {
                        PPSE.to_vec()
                    } else {
                        parse_hex(v).ok_or("--aid needs hex or `ppse`")?
                    });
                }
                other => return Err(format!("unknown option {other:?}")),
            }
        }
        Ok(options)
    }

    fn matches(&self, info: &ccid::ReaderInfo) -> bool {
        self.ids
            .is_none_or(|(v, p)| info.vendor_id() == v && info.product_id() == p)
            && self.interface.is_none_or(|i| info.interface_number() == i)
    }
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}
