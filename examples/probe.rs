//! Probe CCID readers.
//!
//! ```text
//! cargo run --example probe              # list CCID interfaces
//! cargo run --example probe -- open      # open the first interface
//! cargo run --example probe -- open --reader 072f:2401 --interface 0
//! cargo run --example probe -- read      # power on the card, print ATR and UID
//! ```
//!
//! `open` and `read` take the same `--reader VID:PID` and `--interface N`
//! options; by default they use the first CCID interface found.

use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("list") => list(),
        Some("open") => open(&args[1..]),
        Some("read") => read(&args[1..]),
        Some(other) => {
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

/// Find the reader selected by the options.
fn select(args: &[String]) -> Result<ccid::ReaderInfo, ccid::Error> {
    let filter = Filter::parse(args);
    let info = ccid::list_readers()?
        .into_iter()
        .find(|r| filter.matches(r))
        .ok_or(ccid::Error::NotFound)?;
    println!(
        "reader   {:04x}:{:04x} interface {} \"{}\"",
        info.vendor_id(),
        info.product_id(),
        info.interface_number(),
        info.interface_name().unwrap_or("?"),
    );
    Ok(info)
}

fn open(args: &[String]) -> Result<(), ccid::Error> {
    let reader = select(args)?.open()?;
    let level = reader
        .descriptor()
        .exchange_level()
        .map_or("undefined".to_owned(), |l| l.to_string());
    println!("claimed: {reader:?}");
    println!("exchange level: {level}");
    Ok(())
}

/// Power on the card in slot 0 and read its ATR and UID.
fn read(args: &[String]) -> Result<(), ccid::Error> {
    let mut reader = ccid::blocking::Reader::from(select(args)?.open()?);
    let start = Instant::now();
    let mut card = reader.connect(0)?;
    let powered = start.elapsed();
    let atr = card.atr().clone();
    println!("ATR      {atr}");
    println!("type     {}", atr.card_type());
    let start = Instant::now();
    match card.get_uid() {
        Ok(uid) => println!("UID      {} ({} bytes)", hex(&uid), uid.len()),
        Err(e) => println!("UID      error: {e}"),
    }
    let uid_time = start.elapsed();
    println!(
        "latency  power on {:.1} ms, UID {:.1} ms",
        powered.as_secs_f64() * 1e3,
        uid_time.as_secs_f64() * 1e3
    );
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Reader selection from `--reader VID:PID` and `--interface N`.
#[derive(Default)]
struct Filter {
    ids: Option<(u16, u16)>,
    interface: Option<u8>,
}

impl Filter {
    fn parse(args: &[String]) -> Filter {
        let mut filter = Filter::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--reader" => {
                    let value = it.next().expect("--reader needs VID:PID");
                    let (vid, pid) = value.split_once(':').expect("--reader needs VID:PID");
                    let hex = |s| u16::from_str_radix(s, 16).expect("VID and PID are hex");
                    filter.ids = Some((hex(vid), hex(pid)));
                }
                "--interface" => {
                    let value = it.next().expect("--interface needs a number");
                    filter.interface = Some(value.parse().expect("interface is a number"));
                }
                other => panic!("unknown option {other:?}"),
            }
        }
        filter
    }

    fn matches(&self, info: &ccid::ReaderInfo) -> bool {
        self.ids
            .is_none_or(|(v, p)| info.vendor_id() == v && info.product_id() == p)
            && self.interface.is_none_or(|i| info.interface_number() == i)
    }
}
