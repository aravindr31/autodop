//! Parse a real Deposit-Accounts PDF and show what the importer would take.
//!     cargo run --example pdf_probe -- /path/to/Agent.pdf
use std::path::PathBuf;

fn main() {
    let path = std::env::args().nth(1).expect("a pdf path");
    let text = pdf_extract::extract_text(&path).expect("extract");
    let mut rows = 0;
    let mut carry = String::new();
    for line in text.lines() {
        let combined = if carry.is_empty() {
            line.to_string()
        } else {
            format!("{carry} {line}")
        };
        let data_ish = line
            .split_whitespace()
            .any(|t| t.len() >= 9 && t.chars().all(|c| c.is_ascii_digit()));
        match autodop_lib::parse_deposit_row(&combined) {
            Ok(Some((number, name, denom))) => {
                rows += 1;
                if rows <= 7 || rows > 155 {
                    println!("{number} | {name} | {denom}");
                }
                carry.clear();
            }
            Ok(None) => {
                carry = if data_ish && !line.contains("Cr.") {
                    combined
                } else {
                    String::new()
                };
            }
            Err(_) => {
                println!("UNPARSED: {combined}");
                carry.clear();
            }
        }
    }
    println!("total rows: {rows}");
}
