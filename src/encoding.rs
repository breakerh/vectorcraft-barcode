//! Validated linear barcode encoding. UPC-E is expanded to UPC-A for its check digit.
use barcoders::sym::{
    code39::Code39,
    code93::Code93,
    code128::Code128,
    ean8::EAN8,
    ean13::{EAN13, ENCODINGS},
    tf::TF,
};

pub struct Encoded {
    pub bits: Vec<u8>,
    pub text: String,
}
fn check_digit(data: &str) -> u8 {
    let sum: u32 = data
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, c)| (c - b'0') as u32 * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    ((10 - sum % 10) % 10) as u8
}
fn numeric(data: &str) -> Result<(), String> {
    if data.is_empty() || !data.bytes().all(|b| b.is_ascii_digit()) {
        Err("This barcode type accepts digits 0-9 only; do not include spaces or hyphens.".into())
    } else {
        Ok(())
    }
}
fn gtin(data: &str, body_len: usize) -> Result<String, String> {
    numeric(data)?;
    if data.len() != body_len && data.len() != body_len + 1 {
        return Err(format!(
            "Enter {body_len} digits without a check digit, or {} digits including it.",
            body_len + 1
        ));
    }
    let body = &data[..body_len];
    let check = char::from(b'0' + check_digit(body));
    if data.len() == body_len + 1 && data.as_bytes()[body_len] != check as u8 {
        return Err(format!(
            "Invalid check digit: expected {check}. The supplied value was not changed."
        ));
    }
    Ok(format!("{body}{check}"))
}
fn encoded(bits: Vec<u8>, text: String) -> Result<Encoded, String> {
    Ok(Encoded { bits, text })
}

pub fn encode(kind: &str, data: &str) -> Result<Encoded, String> {
    if data.is_empty() {
        return Err("Enter barcode data.".into());
    }
    if data.len() > 80 {
        return Err(
            "Use at most 80 ASCII characters. Longer linear barcodes become impractically wide."
                .into(),
        );
    }
    if !data.is_ascii() {
        return Err("Use ASCII characters only; accented letters and emoji are not supported by these barcode types.".into());
    }
    let invalid = |e| format!("Invalid data for {kind}: {e:?}.");
    match kind {
        "EAN-13" | "ISBN-13" => {
            let text = gtin(data, 12)?;
            if kind == "ISBN-13" && !text.starts_with("978") && !text.starts_with("979") {
                return Err("ISBN-13 must start with 978 or 979.".into());
            }
            encoded(EAN13::new(&text).map_err(invalid)?.encode(), text)
        }
        "EAN-8" => {
            let text = gtin(data, 7)?;
            encoded(EAN8::new(&text).map_err(invalid)?.encode(), text)
        }
        "UPC-A" => {
            let text = gtin(data, 11)?;
            encoded(
                EAN13::new(format!("0{text}")).map_err(invalid)?.encode(),
                text,
            )
        }
        "UPC-E" => upce(data),
        "Code 128" => {
            if !data.bytes().all(|b| (32..=126).contains(&b)) {
                return Err(
                    "Code 128 accepts printable ASCII characters (space through ~).".into(),
                );
            }
            // Use the double-density C set for all-numeric, even-length input; otherwise use B.
            let start = if data.len() % 2 == 0 && data.bytes().all(|b| b.is_ascii_digit()) {
                '\u{0106}'
            } else {
                '\u{0181}'
            };
            encoded(
                Code128::new(format!("{start}{data}"))
                    .map_err(invalid)?
                    .encode(),
                data.into(),
            )
        }
        "Code 39" | "Code 93" => {
            if !data
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b" -.$/+%".contains(&b))
            {
                return Err("Code 39/93 accepts A-Z, 0-9, space, and - . $ / + %. Do not include start/stop characters.".into());
            }
            let bits = if kind == "Code 39" {
                Code39::new(data).map_err(invalid)?.encode()
            } else {
                Code93::new(data).map_err(invalid)?.encode()
            };
            encoded(bits, data.into())
        }
        "ITF" => {
            numeric(data)?;
            if data.len() < 2 || data.len() % 2 != 0 {
                return Err("ITF requires an even number of digits (at least 2). No check digit is added automatically.".into());
            }
            encoded(
                TF::interleaved(data).map_err(invalid)?.encode(),
                data.into(),
            )
        }
        "ITF-14" => {
            let text = gtin(data, 13)?;
            encoded(TF::interleaved(&text).map_err(invalid)?.encode(), text)
        }
        "Codabar" => {
            let bytes = data.as_bytes();
            let has_start = b"ABCD".contains(&bytes[0]);
            let has_end = b"ABCD".contains(bytes.last().unwrap());
            let (body, text) = if has_start || has_end {
                if !has_start || !has_end || bytes.len() < 3 {
                    return Err(
                        "Use both Codabar start and stop characters (A-D), or omit both.".into(),
                    );
                }
                (&data[1..data.len() - 1], data.to_string())
            } else {
                (data, format!("A{data}A"))
            };
            if body.is_empty()
                || !body
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b"-$:/.+".contains(&b))
            {
                return Err("Codabar accepts digits and - $ : / . +, with optional A-D start/stop characters.".into());
            }
            encoded(codabar_bits(&text), text)
        }
        _ => Err("Choose a supported barcode type.".into()),
    }
}

// EN 798 Codabar wide/narrow patterns, in digit/punctuation/start-stop order.
// Keep this small encoder local: barcoders 2.0 swaps the B and C start/stop patterns.
fn codabar_bits(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"0123456789-$:/.+ABCD";
    const PATTERNS: [u8; 20] = [
        0x03, 0x06, 0x09, 0x60, 0x12, 0x42, 0x21, 0x24, 0x30, 0x48, 0x0c, 0x18, 0x45, 0x51, 0x54,
        0x15, 0x1a, 0x29, 0x0b, 0x0e,
    ];
    let mut bits = Vec::new();
    for (i, ch) in text.bytes().enumerate() {
        if i > 0 {
            bits.push(0);
        }
        let pattern = PATTERNS[ALPHABET.iter().position(|c| *c == ch).unwrap()];
        for j in 0..7 {
            let width = if pattern & (1 << (6 - j)) != 0 { 2 } else { 1 };
            bits.extend(std::iter::repeat_n(if j % 2 == 0 { 1 } else { 0 }, width));
        }
    }
    bits
}

fn upce(data: &str) -> Result<Encoded, String> {
    numeric(data)?;
    let full=match data.len() {6=>format!("0{data}"),7|8=>data.to_string(),_=>return Err("UPC-E requires 6 digits (number system 0), 7 digits (number system plus payload), or 8 digits including the check digit.".into())};
    let b = full.as_bytes();
    if !b"01".contains(&b[0]) {
        return Err("UPC-E number system must be 0 or 1.".into());
    }
    let d = &b[1..7];
    let mut expanded = [b'0'; 11];
    expanded[0] = b[0];
    expanded[1] = d[0];
    expanded[2] = d[1];
    match d[5] {
        b'0'..=b'2' => {
            expanded[3] = d[5];
            expanded[8] = d[2];
            expanded[9] = d[3];
            expanded[10] = d[4];
        }
        b'3' => {
            if d[2] < b'3' {
                return Err("For UPC-E ending in 3, the third payload digit must be 3-9.".into());
            }
            expanded[3] = d[2];
            expanded[9] = d[3];
            expanded[10] = d[4];
        }
        b'4' => {
            if d[3] == b'0' {
                return Err(
                    "For UPC-E ending in 4, the fourth payload digit cannot be zero.".into(),
                );
            }
            expanded[3] = d[2];
            expanded[4] = d[3];
            expanded[10] = d[4];
        }
        _ => {
            if d[4] == b'0' {
                return Err(
                    "For UPC-E ending in 5-9, the fifth payload digit cannot be zero.".into(),
                );
            }
            expanded[3] = d[2];
            expanded[4] = d[3];
            expanded[5] = d[4];
            expanded[10] = d[5];
        }
    }
    let check = check_digit(std::str::from_utf8(&expanded).unwrap());
    if full.len() == 8 && b[7] != b'0' + check {
        return Err(format!("Invalid UPC-E check digit: expected {check}."));
    }
    // UPC-E parity patterns from ISO/IEC 15420. 1 selects the even (G/B) digit encoding.
    const PARITY: [[usize; 6]; 10] = [
        [1, 1, 1, 0, 0, 0],
        [1, 1, 0, 1, 0, 0],
        [1, 1, 0, 0, 1, 0],
        [1, 1, 0, 0, 0, 1],
        [1, 0, 1, 1, 0, 0],
        [1, 0, 0, 1, 1, 0],
        [1, 0, 0, 0, 1, 1],
        [1, 0, 1, 0, 1, 0],
        [1, 0, 1, 0, 0, 1],
        [1, 0, 0, 1, 0, 1],
    ];
    let mut bits = vec![1, 0, 1];
    for (i, digit) in d.iter().enumerate() {
        let parity = PARITY[check as usize][i] ^ usize::from(b[0] == b'1');
        bits.extend_from_slice(&ENCODINGS[parity][(digit - b'0') as usize]);
    }
    bits.extend_from_slice(&[0, 1, 0, 1, 0, 1]);
    encoded(bits, format!("{}{check}", &full[..7]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gtin_checks_are_verified() {
        for (kind, body, full) in [
            ("EAN-13", "590123412345", "5901234123457"),
            ("EAN-8", "9638507", "96385074"),
            ("UPC-A", "03600029145", "036000291452"),
            ("ISBN-13", "978030640615", "9780306406157"),
            ("ITF-14", "1001234500001", "10012345000017"),
        ] {
            assert_eq!(encode(kind, body).unwrap().text, full);
            assert_eq!(
                encode(kind, body).unwrap().bits,
                encode(kind, full).unwrap().bits
            );
            let mut wrong = full.to_string();
            wrong.pop();
            wrong.push('9');
            assert!(encode(kind, &wrong).is_err());
        }
    }
    #[test]
    fn upce_vectors_and_restrictions() {
        assert_eq!(encode("UPC-E", "425261").unwrap().text, "04252614");
        assert_eq!(encode("UPC-E", "04252614").unwrap().bits.len(), 51);
        for bad in ["2425261", "04252615", "121003", "123004", "123405"] {
            assert!(encode("UPC-E", bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn invalid_input_is_never_changed_silently() {
        for (kind, bad) in [
            ("EAN-13", "590 1234123457"),
            ("EAN-8", "１２３４５６７"),
            ("Code 39", "lowercase"),
            ("Code 93", "*ABC*"),
            ("Code 128", "hello\nworld"),
            ("Code 128", "café"),
            ("ITF", "123"),
            ("Codabar", "A123"),
            ("Codabar", "A12B34A"),
            ("ISBN-13", "5901234123457"),
        ] {
            assert!(encode(kind, bad).is_err(), "{kind}: {bad}");
        }
    }
    #[test]
    fn code128_uses_numeric_set_and_keeps_spaces() {
        assert_eq!(encode("Code 128", "123456").unwrap().bits.len(), 68);
        assert_eq!(
            encode("Code 128", "  Example-123  ").unwrap().text,
            "  Example-123  "
        );
    }
    #[test]
    fn codabar_frames_payload_once() {
        assert_eq!(encode("Codabar", "12345").unwrap().text, "A12345A");
        assert_eq!(encode("Codabar", "B12345D").unwrap().text, "B12345D");
    }
}
