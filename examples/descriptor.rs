//! Dump the CCID class descriptor of every CCID interface.
//!
//! This does not claim any interface, so it also works on macOS while
//! `usbsmartcardreaderd` holds the reader.
//!
//! ```text
//! cargo run --example descriptor
//! ```

use ccid::CcidDescriptor;

fn main() -> Result<(), ccid::Error> {
    let readers = ccid::list_readers()?;
    if readers.is_empty() {
        println!("no CCID interface found");
    }
    for info in readers {
        println!(
            "{:04x}:{:04x} interface {} \"{}\" ({})",
            info.vendor_id(),
            info.product_id(),
            info.interface_number(),
            info.interface_name().unwrap_or("?"),
            info.product().unwrap_or("?"),
        );
        match info.descriptor() {
            Ok(d) => print_descriptor(&d),
            Err(e) => println!("  error: {e}"),
        }
        println!();
    }
    Ok(())
}

fn print_descriptor(d: &CcidDescriptor) {
    let level = d
        .exchange_level()
        .map_or("undefined".to_owned(), |l| l.to_string());
    let features: Vec<_> = d.features.names().collect();
    println!(
        "  bcdCCID         {:x}.{:02x}",
        d.bcd_ccid >> 8,
        d.bcd_ccid & 0xff
    );
    println!("  slots           {}", d.slot_count());
    println!("  voltages        {}", d.voltages);
    println!("  protocols       {}", d.protocols);
    println!(
        "  clock           {} kHz default, {} kHz max",
        d.default_clock_khz, d.max_clock_khz
    );
    println!(
        "  data rate       {} bit/s default, {} bit/s max",
        d.data_rate, d.max_data_rate
    );
    println!("  max IFSD        {}", d.max_ifsd);
    println!("  features        0x{:08x}", d.features.0);
    for name in features {
        println!("                  - {name}");
    }
    println!("  exchange level  {level}");
    println!("  max message     {} bytes", d.max_message_length);
    println!(
        "  GET RESPONSE / ENVELOPE class  0x{:02x} / 0x{:02x}",
        d.class_get_response, d.class_envelope
    );
    println!("  PIN support     0x{:02x}", d.pin_support);
    println!("  max busy slots  {}", d.max_busy_slots);
}
