//! Helper utilities.

use bytes::BufMut;

/// Appends `bytes` to `buf` as a token or a quoted-string.
///
/// # Important
///
/// This function ignores control characters (`%x00-08 / %x0A-1F / %x7F`) as those are not valid in
/// either `token` or `quoted-string`. It's the caller's responsibility to ensure the input does
/// not contain these bytes.
pub(super) fn put_token_or_quoted<B: BufMut>(buf: &mut B, bytes: impl AsRef<[u8]>) {
    let mut bytes = bytes.as_ref();
    if bytes.is_empty() {
        buf.put_slice(b"\"\"");
        return;
    }
    match bytes.iter().position(|&b| !is_tchar(b)) {
        Some(mut skip) => {
            // quoted-string
            buf.put_u8(b'"');
            while let Some(idx) = bytes
                .iter()
                .skip(skip)
                .position(|&b| matches!(b, b'"' | b'\\'))
            {
                let chunk = take(&mut bytes, ..idx + skip);
                buf.put_slice(chunk);
                buf.put_u8(b'\\');
                skip = 1;
            }
            buf.put_slice(bytes);
            buf.put_u8(b'"');
        }
        None => {
            // token
            buf.put_slice(bytes);
        }
    }
}

fn is_tchar(b: u8) -> bool {
    // https://httpwg.org/specs/rfc7230.html#rule.token.separators
    matches!(b,
          b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`'  | b'|' | b'~'
        | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
    )
}

// stable version of `slice::take()`
fn take<'a>(s: &mut &'a [u8], range: std::ops::RangeTo<usize>) -> &'a [u8] {
    let (head, tail) = s.split_at(range.end);
    *s = tail;
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_output(bytes: impl AsRef<[u8]>, expected: impl AsRef<[u8]>) {
        let mut buf = Vec::new();
        put_token_or_quoted(&mut buf, bytes);
        assert_eq!(ByteStr(&buf), ByteStr(expected.as_ref()));
    }

    #[test]
    fn test_empty() {
        assert_output("", "\"\"");
    }

    #[test]
    fn test_token() {
        #[track_caller]
        fn assert_token(bytes: impl AsRef<[u8]>) {
            let bytes = bytes.as_ref();
            assert_output(bytes, bytes);
        }
        assert_token("a");
        assert_token("!#$%'*+-.^_`|~");
        assert_token("some.WORDS.and.239zaq");
        assert_token("127.0.0.1");
    }

    #[test]
    fn test_quoted_string() {
        assert_output(" ", r#"" ""#);
        assert_output("one\ttwo", "\"one\ttwo\"");
        assert_output("\"quoted\\text\"", r#""\"quoted\\text\"""#);
        assert_output("🦌", r#""🦌""#);
    }

    /// Helper to print test failures in a more readable fashion.
    ///
    /// This prints as much utf-8 as possible, printing `\x##` escapes for non-utf8 bytes. It does
    /// not escape any characters and omits surrounding double quotes, since the contained string
    /// likely has backslash escapes and quotes in it and we want to make it easier to read.
    ///
    /// There is potential ambiguity between a string that looks like `\x##` and a non-utf8 byte,
    /// but we're not expecting to run into that.
    struct ByteStr<'a>(&'a [u8]);
    impl std::fmt::Debug for ByteStr<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut bytes = self.0;
            loop {
                let err = match std::str::from_utf8(bytes) {
                    Ok(s) => return f.write_str(s),
                    Err(err) => err,
                };
                f.write_str(std::str::from_utf8(take(&mut bytes, ..err.valid_up_to())).unwrap())?;
                match err.error_len() {
                    None => {
                        for &b in bytes {
                            write!(f, "{}", std::ascii::escape_default(b))?;
                        }
                        return Ok(());
                    }
                    Some(len) => {
                        for &b in take(&mut bytes, ..len) {
                            write!(f, "{}", std::ascii::escape_default(b))?;
                        }
                    }
                };
            }
        }
    }
    impl PartialEq for ByteStr<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
}
