use std::ops::RangeInclusive;

/// Function that parses the content of a range-header
/// If correctly formatted returns the requested ranges
/// If syntactically correct but unsatisfiable due to file-constraints returns `Unsatisfiable`
/// If un-parseable as a range returns `Malformed`
/// Caveats:
///     Does not handle multipart ranges
///     Only accepts bytes ranges
fn parse_range(range: &str, target_file_length: u64) -> ParsedRangeHeader {
    let start = range.split_once("bytes=");
    // ("", 0-1023) or ("", 0-)
    if let Some((_, indicated_range)) = start {
        let mut ranges = Vec::new();
        for range in indicated_range.split(",") {
            let sep_count = range.find_substring("-").unwrap_or(0);
            if sep_count != 1 {
                return ParsedRangeHeader::Malformed;
            }
            if let Some((start, end)) = range.split_once("-") {
                if start == "" {
                    if end >= target_file_length {
                        return ParsedRangeHeader::Unsatisfiable;
                    } else {
                        ranges.push(RangeInclusive::new(0, target_file_length - 1));
                        break;
                    }
                }
                if let Ok(start) = start.parse::<u64>() {
                    if end == "" {
                        ranges.push(RangeInclusive::new(start, target_file_length - 1));
                        break;
                    }
                    if let Ok(end) = end.parse::<u64>() {

                    } else {
                        return ParsedRangeHeader::Malformed;
                    }
                } else {
                    return ParsedRangeHeader::Malformed;
                }
            }
        }
    }
    ParsedRangeHeader::Malformed
}

#[derive(Debug)]
pub(crate) enum ParsedRangeHeader {
    Range(Vec<RangeInclusive<u64>>),
    Unsatisfiable,
    Malformed,
}

#[cfg(test)]
mod tests {
    use crate::services::fs::parse_range::parse_range;

    #[test]
    fn parse_standard_range() {
        let input = "bytes=0-1023";
        println!("{:?}", parse_range(input, 10000));
    }
}
