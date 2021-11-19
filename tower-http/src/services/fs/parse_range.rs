use std::ops::{RangeInclusive, RangeBounds};

/// Function that parses the content of a range-header
/// If correctly formatted returns the requested ranges
/// If syntactically correct but unsatisfiable due to file-constraints returns `Unsatisfiable`
/// If un-parseable as a range returns `Malformed`
/// Example:
///
fn parse_range(unit_token: &str, range: &str, target_file_length: u64) -> ParsedRangeHeader {
    let start = range.split_once(format!("{}=", unit_token));
    let mut ranges = Vec::new();
    if let Some((_, indicated_range)) = start {
        for range in indicated_range.split(",") {
            let range = range.trim();
            let sep_count = range.match_indices("-").count();
            if sep_count != 1 {
                return ParsedRangeHeader::Malformed;
            }
            if let Some((start, end)) = range.split_once("-") {
                if start == "" {
                    if let Ok(end) = end.parse::<u64>() {
                        if end >= target_file_length || end == 0{
                            return ParsedRangeHeader::Unsatisfiable;
                        } else {
                            let start = target_file_length - 1 - end;
                            ranges.push(RangeInclusive::new(start, target_file_length - 1));
                            continue;
                        }
                    } else {
                        return ParsedRangeHeader::Malformed;
                    }
                } else if let Ok(start) = start.parse::<u64>() {
                    if end == "" {
                        ranges.push(RangeInclusive::new(start, target_file_length - 1));
                        continue;
                    }
                    if let Ok(end) = end.parse::<u64>() {
                        if end >= target_file_length {
                            return ParsedRangeHeader::Unsatisfiable;
                        } else {
                            ranges.push(RangeInclusive::new(start, end));
                            continue;
                        }

                    } else {
                        return ParsedRangeHeader::Malformed;
                    }
                } else {
                    return ParsedRangeHeader::Malformed;
                }
            }
        }
    }
    if ranges.is_empty() {
        ParsedRangeHeader::Malformed
    } else {
        if ranges.len() == 1 {
            ParsedRangeHeader::Range(ranges)
        } else if !overlaps(&ranges) {
            ParsedRangeHeader::Range(ranges)
        } else {
            ParsedRangeHeader::Unsatisfiable
        }
    }
}

fn overlaps(ranges: &Vec<RangeInclusive<u64>>) -> bool {
    let mut bounds = Vec::new();
    for range in ranges {
        bounds.push((range.start(), range.end()));
    }
    for i in 0..bounds.len() {
        for j in i + 1..bounds.len() {
            if bounds[i].0 <= bounds[j].1 && bounds[j].0 <= bounds[i].1 {
                return true
            }
        }
    }
    false
}

#[derive(Debug, PartialEq)]
pub(crate) enum ParsedRangeHeader {
    Range(Vec<RangeInclusive<u64>>),
    Unsatisfiable,
    Malformed,
}

#[cfg(test)]
mod tests {
    use crate::services::fs::parse_range::{parse_range, ParsedRangeHeader};
    use std::ops::RangeInclusive;
    const TEST_FILE_LENGTH: u64 = 10_000;

    #[test]
    fn parse_standard_range() {
        let input = "bytes=0-1023";
        assert_eq!(ParsedRangeHeader::Range(vec![RangeInclusive::new(0, 1023)]), parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_open_ended_range() {
        let input = &format!("bytes=0-{}", TEST_FILE_LENGTH - 1);
        assert_eq!(ParsedRangeHeader::Range(vec![RangeInclusive::new(0, TEST_FILE_LENGTH - 1)]), parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_suffix_range() {
        let input = "bytes=-15";
        assert_eq!(ParsedRangeHeader::Range(vec![RangeInclusive::new(TEST_FILE_LENGTH - 15 - 1, TEST_FILE_LENGTH - 1)]), parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_empty_as_malformed() {
        let input = "bytes=";
        assert_eq!(ParsedRangeHeader::Malformed, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_bad_unit_as_malformed() {
        let input = "abcde=0-10";
        assert_eq!(ParsedRangeHeader::Malformed, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_missing_equals_as_malformed() {
        let input = "abcde0-10";
        assert_eq!(ParsedRangeHeader::Malformed, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_negative_bad_characters_in_range_as_malformed() {
        let input = "bytes=1-10a";
        assert_eq!(ParsedRangeHeader::Malformed, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_negative_numbers_as_malformed() {
        let input = "bytes=-1-10";
        assert_eq!(ParsedRangeHeader::Malformed, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_out_of_bounds_overrun_as_unsatisfiable() {
        let input = &format!("bytes=0-{}", TEST_FILE_LENGTH);
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_out_of_bounds_suffix_overrun_as_unsatisfiable() {
        let input = &format!("bytes=-{}", TEST_FILE_LENGTH);
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_zero_length_suffix_as_unsatisfiable() {
        let input = &format!("bytes=-0");
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
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
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_open() {
        let input = "bytes=0-, 5000-6000";
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_suffixed() {
        let input = "bytes=8000-9000, -1001";
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
    }

    #[test]
    fn parse_overlapping_multi_range_as_unsatisfiable_suffixed_open() {
        let input = "bytes=0-, -1";
        assert_eq!(ParsedRangeHeader::Unsatisfiable, parse_range(input, TEST_FILE_LENGTH));
    }
}
