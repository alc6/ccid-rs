//! ACS vendor commands on an ACR1552U-family reader (feature `acs`).
//!
//! ```text
//! cargo run --example acs --features acs                          # show settings
//! cargo run --example acs --features acs -- beep 100              # beep for 100 ms
//! cargo run --example acs --features acs -- led blue|green|both|off
//! cargo run --example acs --features acs -- rf-off-interval 250|500|1000|2500|none
//! cargo run --example acs --features acs -- rf off|on|on-no-polling
//! cargo run --example acs --features acs -- beep-on-arrival on|off
//! cargo run --example acs --features acs -- pause 5   # field off 5 s, then on 5 s, printing card events
//! ```
//!
//! It uses the first CCID interface found, which on the ACR1552U is the
//! contactless one. The polling option and the UI behaviour are saved in
//! the reader.

use std::process::ExitCode;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};

use ccid::acs::{Leds, RfMode, RfOffInterval};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let info = ccid::list_readers()?
        .into_iter()
        .next()
        .ok_or(ccid::Error::NotFound)?;
    let mut reader = ccid::blocking::Reader::open(&info)?;
    if let ["pause", secs] = args {
        return pause(&mut reader, Duration::from_secs(secs.parse()?));
    }
    let mut acs = reader.acs();
    match args {
        [] | ["info"] => {
            println!("firmware         {}", acs.firmware_version()?);
            println!("serial number    {}", acs.serial_number()?);
            println!("LEDs             {:?}", acs.leds()?);
            println!("field            {:?}", acs.field_status()?);
            let option = acs.polling_option()?;
            println!("polling option   0x{:02X}", option.0);
            println!("  polling        {}", option.polling());
            println!(
                "  RF off interval {:?}",
                option.rf_off_interval().map(|i| i.duration())
            );
            println!("  MIFARE type in ATR {}", option.mifare_type_in_atr());
            let ui = acs.ui_behaviour()?;
            println!("UI behaviour     0x{:02X}", ui.0);
            println!("  beep on arrival {}", ui.beep_on_arrival());
            println!("  beep on removal {}", ui.beep_on_removal());
        }
        ["beep", rest @ ..] => {
            let ms = rest.first().map_or(Ok(100), |s| s.parse())?;
            acs.beep(Duration::from_millis(ms))?;
            println!("beeped for {ms} ms");
        }
        ["led", which] => {
            let leds = match *which {
                "blue" => Leds {
                    blue: true,
                    green: false,
                },
                "green" => Leds {
                    blue: false,
                    green: true,
                },
                "both" => Leds {
                    blue: true,
                    green: true,
                },
                "off" => Leds::default(),
                _ => return Err("led takes blue, green, both or off".into()),
            };
            println!("LEDs now {:?}", acs.set_leds(leds)?);
        }
        ["rf-off-interval", value] => {
            let interval = match *value {
                "250" => Some(RfOffInterval::Ms250),
                "500" => Some(RfOffInterval::Ms500),
                "1000" => Some(RfOffInterval::Ms1000),
                "2500" => Some(RfOffInterval::Ms2500),
                "none" => None,
                _ => return Err("rf-off-interval takes 250, 500, 1000, 2500 or none".into()),
            };
            let before = acs.polling_option()?;
            let after = acs.set_polling_option(before.with_rf_off_interval(interval))?;
            println!("polling option 0x{:02X} -> 0x{:02X}", before.0, after.0);
        }
        ["rf", mode] => {
            let mode = match *mode {
                "off" => RfMode::Off,
                "on" => RfMode::OnWithPolling,
                "on-no-polling" => RfMode::OnWithoutPolling,
                _ => return Err("rf takes off, on or on-no-polling".into()),
            };
            acs.set_rf_mode(mode)?;
            println!("RF {mode:?}, field now {:?}", acs.field_status()?);
        }
        ["beep-on-arrival", on] => {
            let on = match *on {
                "on" => true,
                "off" => false,
                _ => return Err("beep-on-arrival takes on or off".into()),
            };
            let before = acs.ui_behaviour()?;
            let after = acs.set_ui_behaviour(before.with_beep_on_arrival(on))?;
            println!("UI behaviour 0x{:02X} -> 0x{:02X}", before.0, after.0);
        }
        _ => return Err(format!("unknown command {args:?}").into()),
    }
    Ok(())
}

/// Switch the field off for `time`, then back on for `time`, while printing
/// card events: a card tapped during the pause is not seen.
fn pause(
    reader: &mut ccid::blocking::Reader,
    time: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let events = reader.events()?;
    spawn(move || {
        for event in events {
            println!("{:7.3} s  event {event:?}", start.elapsed().as_secs_f64());
        }
    });
    let mut acs = reader.acs();
    acs.set_rf_mode(RfMode::Off)?;
    println!(
        "{:7.3} s  field {:?}: polling paused for {} s",
        start.elapsed().as_secs_f64(),
        acs.field_status()?,
        time.as_secs()
    );
    sleep(time);
    acs.set_rf_mode(RfMode::OnWithPolling)?;
    println!(
        "{:7.3} s  field {:?}: polling again for {} s",
        start.elapsed().as_secs_f64(),
        acs.field_status()?,
        time.as_secs()
    );
    sleep(time);
    Ok(())
}
