use std::fmt;

use arrayvec::ArrayVec;

use crate::content_encoding::{encodings, Encoding, SupportedEncodings};

pub(crate) enum SupportedEncoding {
    Identity,
    #[cfg(feature = "compression-deflate")]
    Deflate,
    #[cfg(feature = "compression-gzip")]
    Gzip,
    #[cfg(feature = "compression-br")]
    Brotli,
    #[cfg(feature = "compression-zstd")]
    Zstd,
}

/// Holds configuration for which compression to use when there is more than
/// one match between client and server-supported compression algorithms.
#[derive(Clone, Copy)]
pub struct EncodingPreference(EncodingPreferenceInner);

impl EncodingPreference {
    /// Within the highest-quality encodings both client and server support, use
    /// the first one by the order the client sent them in `accept-encoding`.
    pub fn first_supported() -> Self {
        Self(EncodingPreferenceInner::FirstSupported)
    }

    #[track_caller]
    fn list(list: ArrayVec<Encoding, 5>) -> Self {
        Self(EncodingPreferenceInner::List(EncodingPreferenceList::new(
            list,
        )))
    }

    pub(super) fn select(
        &self,
        headers: &http::HeaderMap,
        supported_encodings: impl SupportedEncodings,
    ) -> Encoding {
        let encodings = encodings(headers, supported_encodings);

        let pref = match &self.0 {
            EncodingPreferenceInner::List(list) => {
                encodings.max_by_key(|&(enc, qval)| (qval, list.get(enc)))
            }
            EncodingPreferenceInner::FirstSupported => encodings.max_by_key(|&(_, qval)| qval),
        };

        pref.map(|(enc, _)| enc).unwrap_or(Encoding::Identity)
    }
}

impl Default for EncodingPreference {
    fn default() -> Self {
        Self(EncodingPreferenceInner::List(EncodingPreferenceList {
            identity_priority: EncodingPriority::_1,
            #[cfg(feature = "compression-deflate")]
            deflate_priority: EncodingPriority::_2,
            #[cfg(feature = "compression-gzip")]
            gzip_priority: EncodingPriority::_3,
            #[cfg(feature = "compression-br")]
            brotli_priority: EncodingPriority::_4,
            #[cfg(feature = "compression-zstd")]
            zstd_priority: EncodingPriority::_5,
        }))
    }
}

impl fmt::Debug for EncodingPreference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<[Encoding; 5]> for EncodingPreference {
    #[track_caller]
    fn from(value: [Encoding; 5]) -> Self {
        Self::list(ArrayVec::from(value))
    }
}

impl From<[Encoding; 4]> for EncodingPreference {
    #[track_caller]
    fn from(value: [Encoding; 4]) -> Self {
        Self::list(value.into_iter().collect())
    }
}

impl From<[Encoding; 3]> for EncodingPreference {
    #[track_caller]
    fn from(value: [Encoding; 3]) -> Self {
        Self::list(value.into_iter().collect())
    }
}

impl From<[Encoding; 2]> for EncodingPreference {
    #[track_caller]
    fn from(value: [Encoding; 2]) -> Self {
        Self::list(value.into_iter().collect())
    }
}

impl From<[Encoding; 1]> for EncodingPreference {
    fn from(value: [Encoding; 1]) -> Self {
        Self::list(value.into_iter().collect())
    }
}

impl From<Encoding> for EncodingPreference {
    fn from(value: Encoding) -> Self {
        Self::list([value].into_iter().collect())
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq))]
enum EncodingPreferenceInner {
    List(EncodingPreferenceList),
    FirstSupported,
}

impl fmt::Debug for EncodingPreferenceInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(pref) => f
                .debug_tuple("List")
                .field(&pref.dbg_sorted_descending())
                .finish(),
            Self::FirstSupported => f.debug_tuple("FirstSupported").finish(),
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(PartialEq))]
struct EncodingPreferenceList {
    identity_priority: EncodingPriority,
    #[cfg(feature = "compression-deflate")]
    deflate_priority: EncodingPriority,
    #[cfg(feature = "compression-gzip")]
    gzip_priority: EncodingPriority,
    #[cfg(feature = "compression-br")]
    brotli_priority: EncodingPriority,
    #[cfg(feature = "compression-zstd")]
    zstd_priority: EncodingPriority,
}

impl EncodingPreferenceList {
    #[track_caller]
    fn new(list: ArrayVec<Encoding, 5>) -> Self {
        // Not a closure to support tracking the callsite
        // https://github.com/rust-lang/rust/issues/87417
        let mut next_prio = 5;
        macro_rules! set_prio {
            ($field:ident, $enc:ident) => {{
                if $field.is_some() {
                    panic!("Encoding preference list contains a duplicate: {:?}", $enc);
                }
                $field = Some(EncodingPriority::from_u8(next_prio));
            }};
        }

        const DEFAULTS: [SupportedEncoding; 5] = [
            #[cfg(feature = "compression-zstd")]
            Encoding::Zstd,
            #[cfg(feature = "compression-br")]
            Encoding::Brotli,
            #[cfg(feature = "compression-gzip")]
            Encoding::Gzip,
            #[cfg(feature = "compression-deflate")]
            Encoding::Deflate,
            Encoding::Identity,
        ];

        let mut identity_priority = None;
        #[cfg(feature = "compression-deflate")]
        let mut deflate_priority = None;
        #[cfg(feature = "compression-gzip")]
        let mut gzip_priority = None;
        #[cfg(feature = "compression-br")]
        let mut brotli_priority = None;
        #[cfg(feature = "compression-zstd")]
        let mut zstd_priority = None;

        for enc in list {
            match enc {
                Encoding::Identity => set_prio!(identity_priority, enc),
                #[cfg(feature = "compression-deflate")]
                Encoding::Deflate => set_prio!(deflate_priority, enc),
                #[cfg(feature = "compression-gzip")]
                Encoding::Gzip => set_prio!(gzip_priority, enc),
                #[cfg(feature = "compression-br")]
                Encoding::Brotli => set_prio!(brotli_priority, enc),
                #[cfg(feature = "compression-zstd")]
                Encoding::Zstd => set_prio!(zstd_priority, enc),
            }

            next_prio -= 1;
        }

        for enc in DEFAULTS {
            if next_prio == 0 {
                break;
            }

            match enc {
                Encoding::Identity => {
                    identity_priority.get_or_insert(EncodingPriority::from_u8(next_prio));
                }
                Encoding::Deflate => {
                    deflate_priority.get_or_insert(EncodingPriority::from_u8(next_prio));
                }
                Encoding::Gzip => {
                    gzip_priority.get_or_insert(EncodingPriority::from_u8(next_prio));
                }
                #[cfg(feature = "compression-br")]
                Encoding::Brotli => {
                    brotli_priority.get_or_insert(EncodingPriority::from_u8(next_prio));
                }
                #[cfg(feature = "compression-zstd")]
                Encoding::Zstd => {
                    zstd_priority.get_or_insert(EncodingPriority::from_u8(next_prio));
                }
            }

            next_prio -= 1;
        }

        Self::new_(
            identity_priority,
            deflate_priority,
            gzip_priority,
            brotli_priority,
            #[cfg(feature = "compression-zstd")]
            zstd_priority,
        )
    }

    fn new_(
        identity_priority: Option<EncodingPriority>,
        deflate_priority: Option<EncodingPriority>,
        gzip_priority: Option<EncodingPriority>,
        #[cfg(feature = "compression-br")] brotli_priority: Option<EncodingPriority>,
        zstd_priority: Option<EncodingPriority>,
    ) -> EncodingPreferenceList {
        // Separate function to "neutralize" new's #[track_caller] attribute
        Self {
            identity_priority: identity_priority.unwrap(),
            deflate_priority: deflate_priority.unwrap(),
            gzip_priority: gzip_priority.unwrap(),
            #[cfg(feature = "compression-br")]
            brotli_priority: brotli_priority.unwrap(),
            zstd_priority: zstd_priority.unwrap(),
        }
    }

    fn get(&self, enc: Encoding) -> EncodingPriority {
        match enc {
            Encoding::Identity => self.identity_priority,
            Encoding::Deflate => self.deflate_priority,
            Encoding::Gzip => self.gzip_priority,
            #[cfg(feature = "compression-br")]
            Encoding::Brotli => self.brotli_priority,
            Encoding::Zstd => self.zstd_priority,
        }
    }

    fn dbg_sorted_descending(&self) -> DebugSortedDescendingEncodingPreferenceList {
        let mut result = ArrayVec::new();
        result.extend(self.encoding_for_priority(EncodingPriority::_5));
        result.extend(self.encoding_for_priority(EncodingPriority::_4));
        result.extend(self.encoding_for_priority(EncodingPriority::_3));
        result.extend(self.encoding_for_priority(EncodingPriority::_2));
        result.extend(self.encoding_for_priority(EncodingPriority::_1));
        DebugSortedDescendingEncodingPreferenceList(result)
    }

    fn encoding_for_priority(&self, prio: EncodingPriority) -> Option<Encoding> {
        if self.identity_priority == prio {
            Some(Encoding::Identity)
        } else if self.deflate_priority == prio {
            Some(Encoding::Deflate)
        } else if self.gzip_priority == prio {
            Some(Encoding::Gzip)
        } else if self.brotli_priority == prio {
            Some(Encoding::Brotli)
        } else if self.zstd_priority == prio {
            Some(Encoding::Zstd)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EncodingPriority {
    // lowest priority
    _1,
    _2,
    _3,
    _4,
    // highest priority
    _5,
}

impl EncodingPriority {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::_1,
            2 => Self::_2,
            3 => Self::_3,
            4 => Self::_4,
            5 => Self::_5,
            _ => panic!("internal error: priority out of bounds"),
        }
    }
}

struct DebugSortedDescendingEncodingPreferenceList(ArrayVec<Encoding, 5>);

impl fmt::Debug for DebugSortedDescendingEncodingPreferenceList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let enc = self
            .0
            .first()
            .expect("encoding preference list must be non-empty");
        f.write_str(enc.to_str())?;

        for enc in &self.0[1..] {
            f.write_str(", ")?;
            f.write_str(enc.to_str())?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_encoding_preference() {
        assert_eq!(
            EncodingPreference::list(ArrayVec::new()).0,
            EncodingPreference::default().0
        );
    }

    #[derive(Copy, Clone, Default)]
    struct SupportedEncodingsAll;

    impl SupportedEncodings for SupportedEncodingsAll {
        fn gzip(&self) -> bool {
            true
        }

        fn deflate(&self) -> bool {
            true
        }

        fn br(&self) -> bool {
            true
        }

        fn zstd(&self) -> bool {
            true
        }
    }

    #[test]
    fn no_accept_encoding_header() {
        let encoding = Encoding::from_headers(&http::HeaderMap::new(), SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);
    }

    #[test]
    fn accept_encoding_header_single_encoding() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);
    }

    #[test]
    fn accept_encoding_header_two_encodings() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_gzip_x_gzip() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip,x-gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);
    }

    #[test]
    fn accept_encoding_header_x_gzip_deflate() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("deflate,x-gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);
    }

    #[test]
    fn accept_encoding_header_three_encodings() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip,deflate,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_two_encodings_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_three_encodings_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,deflate,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn two_accept_encoding_headers_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn two_accept_encoding_headers_three_encodings_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,deflate"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn three_accept_encoding_headers_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("deflate"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_two_encodings_with_two_qvalues() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,br;q=0.8"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.8,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.995,br;q=0.999"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_three_encodings_with_three_qvalues() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,deflate;q=0.6,br;q=0.8"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.8,deflate;q=0.6,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.6,deflate;q=0.8,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Deflate, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.995,deflate;q=0.997,br;q=0.999"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_invalid_encdoing() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("invalid,gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);
    }

    #[test]
    fn accept_encoding_header_with_qvalue_zero() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0."),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_with_uppercase_letters() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gZiP"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Gzip, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,br;Q=0.8"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_with_allowed_spaces() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static(" gzip\t; q=0.5 ,\tbr ;\tq=0.8\t"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Brotli, encoding);
    }

    #[test]
    fn accept_encoding_header_with_invalid_spaces() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q =0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q= 0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);
    }

    #[test]
    fn accept_encoding_header_with_invalid_quvalues() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=-0.1"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=00.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5000"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=1.01"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=1.001"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Encoding::Identity, encoding);
    }
}
