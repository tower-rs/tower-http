use std::fmt::Debug;
use std::ops::RangeInclusive;

/// Function that parses the content of a range-header
/// If correctly formatted returns the requested ranges
/// If syntactically correct but unsatisfiable due to file-constraints returns `Unsatisfiable`
/// If un-parseable as a range returns `Malformed`
pub(crate) fn parse_range(range: &str, file_size_bytes: u64) -> ParsedRangeHeader {
    let start = split_once(range, "bytes=");
    let mut ranges = Vec::new();
    if let Some((_, indicated_range)) = start {
        for range in indicated_range.split(",") {
            let range = range.trim();
            let sep_count = range.match_indices("-").count();
            if sep_count != 1 {
                return ParsedRangeHeader::Malformed(format!(
                    "Range: {} is not acceptable, contains multiple dashes (-).",
                    range
                ));
            }
            if let Some((start, end)) = split_once(range, "-") {
                if start == "" {
                    if let Ok(end) = end.parse::<u64>() {
                        if end >= file_size_bytes {
                            return ParsedRangeHeader::Unsatisfiable(format!(
                                "Range: {} is not satisfiable, end of range exceeds file boundary.",
                                range
                            ));
                        }
                        if end == 0 {
                            return ParsedRangeHeader::Unsatisfiable(format!("Range: {} is not satisfiable, suffixed number of bytes to retrieve is zero.", range));
                        }
                        let start = file_size_bytes - 1 - end;
                        ranges.push(RangeInclusive::new(start, file_size_bytes - 1));
                        continue;
                    }
                    return ParsedRangeHeader::Malformed(format!(
                        "Range: {} is not acceptable, end of range not parseable.",
                        range
                    ));
                }
                if let Ok(start) = start.parse::<u64>() {
                    if end == "" {
                        ranges.push(RangeInclusive::new(start, file_size_bytes - 1));
                        continue;
                    }
                    if let Ok(end) = end.parse::<u64>() {
                        if end >= file_size_bytes {
                            return ParsedRangeHeader::Unsatisfiable(format!(
                                "Range: {} is not satisfiable, end of range exceeds file boundary.",
                                range
                            ));
                        }
                        ranges.push(RangeInclusive::new(start, end));
                        continue;
                    }
                    return ParsedRangeHeader::Malformed(format!(
                        "Range: {} is not acceptable, end of range not parseable.",
                        range
                    ));
                }
                return ParsedRangeHeader::Malformed(format!(
                    "Range: {} is not acceptable, start of range not parseable.",
                    range
                ));
            }
            return ParsedRangeHeader::Malformed(format!(
                "Range: {} is not acceptable, range does not contain any dashes.",
                range
            ));
        }
    } else {
        return ParsedRangeHeader::Malformed(format!(
            "Range: {} is not acceptable, range does not start with 'bytes='",
            range
        ));
    }
    if ranges.is_empty() {
        return ParsedRangeHeader::Malformed(format!(
            "Range: {} could not be parsed for unknown reason, please file an issue",
            range
        ));
    } else {
        if ranges.len() == 1 {
            ParsedRangeHeader::Range(ranges)
        } else if !overlaps(&ranges) {
            ParsedRangeHeader::Range(ranges)
        } else {
            return ParsedRangeHeader::Unsatisfiable(format!(
                "Range header: {} is not satisfiable, ranges overlap",
                range
            ));
        }
    }
}

fn overlaps(ranges: &[RangeInclusive<u64>]) -> bool {
    let mut bounds = Vec::new();
    for range in ranges {
        bounds.push((range.start(), range.end()));
    }
    for i in 0..bounds.len() {
        for j in i + 1..bounds.len() {
            if bounds[i].0 <= bounds[j].1 && bounds[j].0 <= bounds[i].1 {
                return true;
            }
        }
    }
    false
}

fn split_once<'a>(s: &'a str, pat: &'a str) -> Option<(&'a str, &'a str)> {
    let mut iter = s.split(pat);
    let left = iter.next()?;
    let right = iter.next()?;
    Some((left, right))
}

#[derive(Debug, PartialEq)]
pub(crate) enum ParsedRangeHeader {
    Range(Vec<RangeInclusive<u64>>),
    Unsatisfiable(String),
    Malformed(String),
}

#[cfg(test)]
mod tests {
    use crate::services::fs::parse_range::{parse_range, ParsedRangeHeader};
    use std::ops::RangeInclusive;
    const TEST_FILE_LENGTH: u64 = 10_000;

    #[test]
    fn parse_standard_range() {
        let input = "bytes=0-1023";
        assert_eq!(
            ParsedRangeHeader::Range(vec![RangeInclusive::new(0, 1023)]),
            parse_range(input, TEST_FILE_LENGTH)
        );
    }

    #[test]
    fn parse_open_ended_range() {
        let input = &format!("bytes=0-{}", TEST_FILE_LENGTH - 1);
        assert_eq!(
            ParsedRangeHeader::Range(vec![RangeInclusive::new(0, TEST_FILE_LENGTH - 1)]),
            parse_range(input, TEST_FILE_LENGTH)
        );
    }

    #[test]
    fn parse_suffix_range() {
        let input = "bytes=-15";
        assert_eq!(
            ParsedRangeHeader::Range(vec![RangeInclusive::new(
                TEST_FILE_LENGTH - 15 - 1,
                TEST_FILE_LENGTH - 1
            )]),
            parse_range(input, TEST_FILE_LENGTH)
        );
    }

    #[test]
    fn parse_empty_as_malformed() {
        let input = "";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Malformed(_)
        ));
    }

    #[test]
    fn parse_empty_range_as_malformed() {
        let input = "bytes=";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Malformed(_)
        ));
    }

    #[test]
    fn parse_bad_unit_as_malformed() {
        let input = "abcde=0-10";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Malformed(_)
        ));
    }

    #[test]
    fn parse_missing_equals_as_malformed() {
        let input = "abcde0-10";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Malformed(_)
        ));
    }

    #[test]
    fn parse_negative_bad_characters_in_range_as_malformed() {
        let input = "bytes=1-10a";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Malformed(_)
        ));
    }

    #[test]
    fn parse_negative_numbers_as_malformed() {
        let input = "bytes=-1-10";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Malformed(_)
        ));
    }

    #[test]
    fn parse_out_of_bounds_overrun_as_unsatisfiable() {
        let input = &format!("bytes=0-{}", TEST_FILE_LENGTH);
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }

    #[test]
    fn parse_out_of_bounds_suffix_overrun_as_unsatisfiable() {
        let input = &format!("bytes=-{}", TEST_FILE_LENGTH);
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }

    #[test]
    fn parse_zero_length_suffix_as_unsatisfiable() {
        let input = &format!("bytes=-0");
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }

    #[test]
    fn parse_multi_range() {
        let input = "bytes=0-1023, 2015-3000, 4000-4500, 8000-9999";
        let expected_ranges = vec![
            RangeInclusive::new(0, 1023),
            RangeInclusive::new(2015, 3000),
            RangeInclusive::new(4000, 4500),
            RangeInclusive::new(8000, 9999),
        ];
        let expect = ParsedRangeHeader::Range(expected_ranges);
        assert_eq!(expect, parse_range(input, 10_000));
    }

    #[test]
    fn parse_multi_range_with_open() {
        let input = "bytes=0-1023, 1024-";
        let expected_ranges = vec![
            RangeInclusive::new(0, 1023),
            RangeInclusive::new(1024, 9999),
        ];
        let expect = ParsedRangeHeader::Range(expected_ranges);
        assert_eq!(expect, parse_range(input, 10_000));
    }

    #[test]
    fn parse_multi_range_with_suffix() {
        let input = "bytes=0-1023, -1000";
        let expected_ranges = vec![
            RangeInclusive::new(0, 1023),
            RangeInclusive::new(8999, 9999),
        ];
        let expect = ParsedRangeHeader::Range(expected_ranges);
        assert_eq!(expect, parse_range(input, 10_000));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_standard() {
        let input = "bytes=0-1023, 500-800";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_open() {
        let input = "bytes=0-, 5000-6000";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_suffixed() {
        let input = "bytes=8000-9000, -1001";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_suffixed_open() {
        let input = "bytes=0-, -1";
        assert!(matches!(
            parse_range(input, TEST_FILE_LENGTH),
            ParsedRangeHeader::Unsatisfiable(_)
        ));
    }
}
