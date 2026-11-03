//! Probe CCID readers.
//!
//! ```text
//! cargo run --example probe              # list CCID interfaces
//! cargo run --example probe -- open      # open the first interface
//! cargo run --example probe -- open --reader 072f:2401 --interface 0
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("list") => list(),
        Some("open") => open(&args[1..]),
        Some(other) => {
            eprintln!("unknown command {other:?}; expected `list` or `open`");
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

fn open(args: &[String]) -> Result<(), ccid::Error> {
    let filter = Filter::parse(args);
    let info = ccid::list_readers()?
        .into_iter()
        .find(|r| filter.matches(r))
        .ok_or(ccid::Error::NotFound)?;
    println!(
        "opening {:04x}:{:04x} interface {} \"{}\"",
        info.vendor_id(),
        info.product_id(),
        info.interface_number(),
        info.interface_name().unwrap_or("?"),
    );
    let reader = info.open()?;
    let level = reader
        .descriptor()
        .exchange_level()
        .map_or("undefined".to_owned(), |l| l.to_string());
    println!("claimed: {reader:?}");
    println!("exchange level: {level}");
    Ok(())
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
