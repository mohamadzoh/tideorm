use super::{binary_json, numeric_text};
use serde_json::json;

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

/// 2020-01-01, in days and in microseconds since PostgreSQL's epoch.
const NEW_YEAR_2020: i32 = 7305;
const NEW_YEAR_2020_AT_TEN: i64 = 7305 * 86_400_000_000 + 10 * 3_600_000_000;

fn interval(micros: i64, days: i32, months: i32) -> Vec<u8> {
    [
        &micros.to_be_bytes()[..],
        &days.to_be_bytes(),
        &months.to_be_bytes(),
    ]
    .concat()
}

/// A length-prefixed value, or a `NULL`.
fn value(bytes: Option<&[u8]>) -> Vec<u8> {
    match bytes {
        Some(bytes) => [&(bytes.len() as i32).to_be_bytes()[..], bytes].concat(),
        None => (-1_i32).to_be_bytes().to_vec(),
    }
}

fn array(lengths: &[i32], elements: &[Option<&[u8]>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in [lengths.len() as i32, 1, 0] {
        bytes.extend(word.to_be_bytes());
    }
    for length in lengths {
        bytes.extend(length.to_be_bytes());
        bytes.extend(1_i32.to_be_bytes());
    }
    for element in elements {
        bytes.extend(value(*element));
    }
    bytes
}

fn range(flags: u8, bounds: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![flags];
    for bound in bounds {
        bytes.extend(value(Some(bound)));
    }
    bytes
}

fn text(type_name: &str, bytes: &[u8]) -> String {
    match binary_json(type_name, bytes) {
        Some(serde_json::Value::String(text)) => text,
        other => panic!("{type_name}: {other:?}"),
    }
}

#[test]
fn intervals_read_as_postgres_writes_iso_8601() {
    let four_five_six = (4 * 3600 + 5 * 60 + 6) * 1_000_000;
    for (micros, days, months, expected) in [
        (0, 0, 0, "PT0S"),
        (four_five_six + 789_000, 3, 14, "P1Y2M3DT4H5M6.789S"),
        (-four_five_six, 3, -14, "P-1Y-2M3DT-4H-5M-6S"),
        (-1, 0, 0, "PT-0.000001S"),
        (1_500_000, 0, 0, "PT1.5S"),
        (100 * 3_600_000_000, 0, 0, "PT100H"),
        (0, -1, 1, "P1M-1D"),
        (i64::MAX, i32::MAX, i32::MAX, "infinity"),
        (i64::MIN, i32::MIN, i32::MIN, "-infinity"),
    ] {
        assert_eq!(text("INTERVAL", &interval(micros, days, months)), expected);
    }
    assert_eq!(binary_json("INTERVAL", &[0; 15]), None);
}

#[test]
fn addresses_read_as_postgres_prints_them() {
    let ipv6 = |segments: [u16; 8]| std::net::Ipv6Addr::from(segments).octets();
    for (bytes, expected) in [
        (
            [&[2, 32, 0, 4][..], &[192, 168, 1, 5]].concat(),
            "192.168.1.5",
        ),
        (
            [&[2, 24, 0, 4][..], &[192, 168, 1, 0]].concat(),
            "192.168.1.0/24",
        ),
        (
            [&[2, 32, 1, 4][..], &[192, 168, 1, 5]].concat(),
            "192.168.1.5/32",
        ),
        (
            [&[3, 128, 0, 16][..], &ipv6([0, 0, 0, 0, 0, 0, 0, 1])].concat(),
            "::1",
        ),
        (
            [
                &[3, 128, 0, 16][..],
                &ipv6([0, 0, 0, 0, 0, 0, 0x0102, 0x0304]),
            ]
            .concat(),
            "::1.2.3.4",
        ),
        (
            [
                &[3, 128, 0, 16][..],
                &ipv6([0, 0, 0, 0, 0, 0xffff, 0x0102, 0x0304]),
            ]
            .concat(),
            "::ffff:1.2.3.4",
        ),
        (
            [
                &[3, 64, 0, 16][..],
                &ipv6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
            ]
            .concat(),
            "2001:db8::1/64",
        ),
    ] {
        assert_eq!(text("INET", &bytes), expected);
    }
}

#[test]
fn zoned_times_and_money_read_exactly() {
    let timetz = |micros: i64, west: i32| [&micros.to_be_bytes()[..], &west.to_be_bytes()].concat();
    for (micros, west, expected) in [
        (37_800_000_000, -7_200, "10:30:00+02"),
        (37_800_250_000, 19_800, "10:30:00.25-05:30"),
        (43_200_000_000, -19_815, "12:00:00+05:30:15"),
        (86_400_000_000, 3_600, "24:00:00-01"),
    ] {
        assert_eq!(text("TIMETZ", &timetz(micros, west)), expected);
    }
    assert_eq!(text("MONEY", &123_456_i64.to_be_bytes()), "1234.56");
    assert_eq!(text("MONEY", &(-1_i64).to_be_bytes()), "-0.01");
    assert_eq!(
        text("MONEY", &i64::MAX.to_be_bytes()),
        "92233720368547758.07"
    );
}

#[test]
fn ranges_read_as_postgres_prints_them() {
    let int4 = |n: i32| n.to_be_bytes();
    assert_eq!(
        text("INT4RANGE", &range(0x02, &[&int4(1), &int4(10)])),
        "[1,10)"
    );
    assert_eq!(text("INT4RANGE", &range(0x01, &[])), "empty");
    assert_eq!(text("INT4RANGE", &range(0x08, &[&int4(5)])), "(,5)");
    assert_eq!(text("INT8RANGE", &range(0x18, &[])), "(,)");
    assert_eq!(
        text(
            "NUMRANGE",
            &range(
                0x06,
                &[&numeric(2, 0, 0, 1, &[1, 5000]), &numeric(1, 0, 0, 0, &[2])]
            )
        ),
        "[1.5,2]"
    );
    assert_eq!(
        text(
            "DATERANGE",
            &range(
                0x02,
                &[&NEW_YEAR_2020.to_be_bytes(), &i32::MAX.to_be_bytes()]
            )
        ),
        "[2020-01-01,infinity)"
    );
    let ten = NEW_YEAR_2020_AT_TEN.to_be_bytes();
    let half_past = (NEW_YEAR_2020_AT_TEN + 1_800_500_000).to_be_bytes();
    assert_eq!(
        text("TSRANGE", &range(0x02, &[&ten, &half_past])),
        "[\"2020-01-01 10:00:00\",\"2020-01-01 10:30:00.5\")"
    );
    assert_eq!(
        text("TSTZRANGE", &range(0x10 | 0x02, &[&ten])),
        "[\"2020-01-01 10:00:00+00\",)"
    );
    // A date before year 1 is ` BC`, after the zone.
    let one_bc = (-730_485_i64 * 86_400_000_000).to_be_bytes();
    assert_eq!(
        text("TSTZRANGE", &range(0x02 | 0x04, &[&one_bc, &one_bc])),
        "[\"0001-01-01 00:00:00+00 BC\",\"0001-01-01 00:00:00+00 BC\"]"
    );
}

#[test]
fn arrays_keep_nulls_dimensions_and_infinite_dates() {
    let int4 = |n: i32| n.to_be_bytes();
    assert_eq!(
        binary_json(
            "INT4[]",
            &array(
                &[2, 2],
                &[Some(&int4(1)), None, Some(&int4(3)), Some(&int4(4))]
            )
        ),
        Some(json!([[1, null], [3, 4]]))
    );
    assert_eq!(binary_json("INT4[]", &array(&[], &[])), Some(json!([])));
    assert_eq!(
        binary_json(
            "DATE[]",
            &array(
                &[3],
                &[
                    Some(&NEW_YEAR_2020.to_be_bytes()),
                    None,
                    Some(&i32::MIN.to_be_bytes())
                ]
            )
        ),
        Some(json!(["2020-01-01", null, "-infinity"]))
    );
    let at_ten = chrono::DateTime::parse_from_rfc3339("2020-01-01T10:00:00+00:00").unwrap();
    assert_eq!(
        binary_json(
            "TIMESTAMPTZ[]",
            &array(&[1], &[Some(&NEW_YEAR_2020_AT_TEN.to_be_bytes())])
        ),
        Some(json!([at_ten]))
    );
    assert_eq!(
        binary_json(
            "TIMESTAMP[]",
            &array(&[1], &[Some(&NEW_YEAR_2020_AT_TEN.to_be_bytes())])
        ),
        Some(json!([at_ten.naive_utc()]))
    );
    assert_eq!(
        binary_json(
            "TIME[]",
            &array(
                &[2],
                &[
                    Some(&1_500_000_i64.to_be_bytes()),
                    Some(&86_400_000_000_i64.to_be_bytes())
                ]
            )
        ),
        Some(json!(["00:00:01.500", "24:00:00"]))
    );
    assert_eq!(
        binary_json(
            "INTERVAL[]",
            &array(&[2], &[Some(&interval(0, 1, 0)), None])
        ),
        Some(json!(["P1D", null]))
    );
    // Bytes that are not an array of the type are no value at all.
    assert_eq!(binary_json("INT4[]", &array(&[1], &[Some(&[1, 2])])), None);
    assert_eq!(binary_json("POINT", &[0; 16]), None);
}
