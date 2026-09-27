use super::numeric_text;

fn numeric(ndigits: u16, weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [ndigits, weight as u16, sign, scale] {
        bytes.extend(word.to_be_bytes());
    }
    for digit in digits {
        bytes.extend(digit.to_be_bytes());
    }
    bytes
}

#[test]
fn numeric_text_reads_the_binary_form_exactly() {
    let thirty_digits = numeric(8, 7, 0, 0, &[12, 3456, 7890, 1234, 5678, 9012, 3456, 7890]);
    assert_eq!(
        numeric_text(&thirty_digits).as_deref(),
        Some("123456789012345678901234567890")
    );
    assert_eq!(
        numeric_text(&numeric(2, 0, 0x4000, 1, &[1, 5000])).as_deref(),
        Some("-1.5")
    );
    assert_eq!(
        numeric_text(&numeric(1, -1, 0, 3, &[500])).as_deref(),
        Some("0.050")
    );
    assert_eq!(
        numeric_text(&numeric(0, 0, 0, 0, &[])).as_deref(),
        Some("0")
    );
    assert_eq!(
        numeric_text(&numeric(0, 0, 0xC000, 0, &[])).as_deref(),
        Some("NaN")
    );
}
