use http::header::HeaderValue;
use httpdate::HttpDate;
use std::time::SystemTime;

/// A strong ETag derived from file metadata (size + mtime with nanosecond precision).
///
/// Format is an implementation detail and may change between versions. Clients should
/// treat ETags as opaque values per RFC 9110 §8.8.3.
#[derive(Clone, Debug)]
pub(super) struct ETag(HeaderValue);

impl ETag {
    /// Generate an ETag from file size and modification time.
    ///
    /// Returns `None` only for pre-epoch modification times, which are unsupported.
    pub(super) fn from_metadata(size: u64, modified: SystemTime) -> Option<Self> {
        let duration = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
        // NOTE: Changing this format is a cache-busting event for all clients,
        // but is not a semver break (ETags are opaque per RFC 9110 §8.8.3).
        let value = format!(
            "\"{:x}.{:08x}-{:x}\"",
            duration.as_secs(),
            duration.subsec_nanos(),
            size
        );
        HeaderValue::from_str(&value).ok().map(ETag)
    }

    pub(super) fn into_header_value(self) -> HeaderValue {
        self.0
    }

    /// Strong comparison per RFC 9110 §8.8.3.2: both must not be weak,
    /// and the opaque-tags must be identical.
    fn strong_eq(&self, other: &[u8]) -> bool {
        if other.starts_with(b"W/") {
            return false;
        }
        self.0.as_bytes() == other
    }

    /// Weak comparison per RFC 9110 §8.8.3.2: ignore W/ prefix,
    /// compare opaque-tags.
    fn weak_eq(&self, other: &[u8]) -> bool {
        let this = self.0.as_bytes();
        let other = other.strip_prefix(b"W/").unwrap_or(other);
        let this = this.strip_prefix(b"W/").unwrap_or(this);
        this == other
    }
}

/// Parsed `If-None-Match` header (RFC 9110 §13.1.2).
pub(super) struct IfNoneMatch(HeaderValue);

impl IfNoneMatch {
    pub(super) fn from_header_value(value: &HeaderValue) -> Option<Self> {
        // Reject empty values
        if value.as_bytes().is_empty() {
            return None;
        }
        Some(IfNoneMatch(value.clone()))
    }

    /// Returns true if the precondition passes (none of the ETags match).
    /// A failed precondition (returns false) means we should return 304.
    ///
    /// Uses weak comparison per RFC 9110 §13.1.2.
    pub(super) fn precondition_passes(&self, etag: &ETag) -> bool {
        let bytes = self.0.as_bytes();
        if bytes == b"*" {
            return false;
        }
        !for_each_etag(bytes, |tag| etag.weak_eq(tag))
    }
}

/// Parsed `If-Match` header (RFC 9110 §13.1.1).
pub(super) struct IfMatch(HeaderValue);

impl IfMatch {
    pub(super) fn from_header_value(value: &HeaderValue) -> Option<Self> {
        if value.as_bytes().is_empty() {
            return None;
        }
        Some(IfMatch(value.clone()))
    }

    /// Returns true if the precondition passes (at least one ETag matches).
    /// A failed precondition (returns false) means we should return 412.
    ///
    /// Uses strong comparison per RFC 9110 §13.1.1.
    pub(super) fn precondition_passes(&self, etag: &ETag) -> bool {
        let bytes = self.0.as_bytes();
        if bytes == b"*" {
            return true;
        }
        for_each_etag(bytes, |tag| etag.strong_eq(tag))
    }
}

/// Iterate over comma-separated ETags in a header value, trimming OWS.
/// Returns true if `predicate` returns true for any tag (short-circuits).
///
/// Handles commas inside quoted strings per RFC 9110 §8.8.3 (ETags are quoted).
fn for_each_etag(header: &[u8], mut predicate: impl FnMut(&[u8]) -> bool) -> bool {
    let mut start = 0;
    let mut in_quotes = false;
    for i in 0..header.len() {
        match header[i] {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                let trimmed = trim_ows(&header[start..i]);
                if !trimmed.is_empty() && predicate(trimmed) {
                    return true;
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let trimmed = trim_ows(&header[start..]);
    if !trimmed.is_empty() && predicate(trimmed) {
        return true;
    }
    false
}

/// Trim leading/trailing OWS (SP / HTAB) per RFC 9110.
fn trim_ows(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t')
        .map(|i| i + 1)
        .unwrap_or(0);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

pub(super) struct LastModified(pub(super) HttpDate);

impl From<SystemTime> for LastModified {
    fn from(time: SystemTime) -> Self {
        LastModified(time.into())
    }
}

pub(super) struct IfModifiedSince(HttpDate);

impl IfModifiedSince {
    /// Check if the supplied time means the resource has been modified.
    pub(super) fn is_modified(&self, last_modified: &LastModified) -> bool {
        self.0 < last_modified.0
    }

    /// Convert a header value into a IfModifiedSince. Invalid values are silently ignored
    pub(super) fn from_header_value(value: &HeaderValue) -> Option<IfModifiedSince> {
        std::str::from_utf8(value.as_bytes())
            .ok()
            .and_then(|value| httpdate::parse_http_date(value).ok())
            .map(|time| IfModifiedSince(time.into()))
    }
}

pub(super) struct IfUnmodifiedSince(HttpDate);

impl IfUnmodifiedSince {
    /// Check if the supplied time passes the precondtion.
    pub(super) fn precondition_passes(&self, last_modified: &LastModified) -> bool {
        self.0 >= last_modified.0
    }

    /// Convert a header value into a IfUnmodifiedSince. Invalid values are silently ignored
    pub(super) fn from_header_value(value: &HeaderValue) -> Option<IfUnmodifiedSince> {
        std::str::from_utf8(value.as_bytes())
            .ok()
            .and_then(|value| httpdate::parse_http_date(value).ok())
            .map(|time| IfUnmodifiedSince(time.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: collect all ETags parsed from a header value.
    fn collect_etags(header: &[u8]) -> Vec<Vec<u8>> {
        let mut tags = Vec::new();
        for_each_etag(header, |tag| {
            tags.push(tag.to_vec());
            false // don't short-circuit, collect all
        });
        tags
    }

    #[test]
    fn for_each_etag_simple_list() {
        let tags = collect_etags(b"\"foo\", \"bar\", \"baz\"");
        assert_eq!(
            tags,
            vec![
                b"\"foo\"".to_vec(),
                b"\"bar\"".to_vec(),
                b"\"baz\"".to_vec()
            ]
        );
    }

    #[test]
    fn for_each_etag_comma_inside_quotes() {
        // An ETag containing a comma inside the quoted string should not be split
        let tags = collect_etags(b"\"foo,bar\", \"baz\"");
        assert_eq!(tags, vec![b"\"foo,bar\"".to_vec(), b"\"baz\"".to_vec()]);
    }

    #[test]
    fn for_each_etag_multiple_commas_inside_quotes() {
        let tags = collect_etags(b"\"a,b,c\", \"d\"");
        assert_eq!(tags, vec![b"\"a,b,c\"".to_vec(), b"\"d\"".to_vec()]);
    }

    #[test]
    fn for_each_etag_weak_with_comma_inside() {
        let tags = collect_etags(b"W/\"foo,bar\", \"baz\"");
        assert_eq!(tags, vec![b"W/\"foo,bar\"".to_vec(), b"\"baz\"".to_vec()]);
    }

    #[test]
    fn for_each_etag_single_tag() {
        let tags = collect_etags(b"\"only\"");
        assert_eq!(tags, vec![b"\"only\"".to_vec()]);
    }

    #[test]
    fn for_each_etag_empty() {
        let tags = collect_etags(b"");
        assert!(tags.is_empty());
    }

    #[test]
    fn for_each_etag_whitespace_only() {
        let tags = collect_etags(b"  ,  , ");
        assert!(tags.is_empty());
    }

    #[test]
    fn for_each_etag_short_circuits() {
        let mut count = 0;
        let found = for_each_etag(b"\"a\", \"b\", \"c\"", |_tag| {
            count += 1;
            count == 2 // match on second tag
        });
        assert!(found);
        assert_eq!(count, 2);
    }
}
