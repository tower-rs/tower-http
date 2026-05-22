use super::{ClassifiedResponse, ClassifyEos, ClassifyResponse, SharedClassifier};
use bitflags::bitflags;
use http::{HeaderMap, Response};
use percent_encoding::percent_decode;
use std::{fmt, num::NonZeroI32};

/// gRPC status codes.
///
/// These variants match the [gRPC status codes].
///
/// [gRPC status codes]: https://github.com/grpc/grpc/blob/master/doc/statuscodes.md#status-codes-and-their-use-in-grpc
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
pub enum GrpcCode {
    /// The operation completed successfully.
    Ok = 0,
    /// The operation was cancelled.
    Cancelled = 1,
    /// Unknown error.
    Unknown = 2,
    /// Client specified an invalid argument.
    InvalidArgument = 3,
    /// Deadline expired before operation could complete.
    DeadlineExceeded = 4,
    /// Some requested entity was not found.
    NotFound = 5,
    /// Some entity that we attempted to create already exists.
    AlreadyExists = 6,
    /// The caller does not have permission to execute the specified operation.
    PermissionDenied = 7,
    /// Some resource has been exhausted.
    ResourceExhausted = 8,
    /// The system is not in a state required for the operation's execution.
    FailedPrecondition = 9,
    /// The operation was aborted.
    Aborted = 10,
    /// Operation was attempted past the valid range.
    OutOfRange = 11,
    /// Operation is not implemented or not supported.
    Unimplemented = 12,
    /// Internal error.
    Internal = 13,
    /// The service is currently unavailable.
    Unavailable = 14,
    /// Unrecoverable data loss or corruption.
    DataLoss = 15,
    /// The request does not have valid authentication credentials
    Unauthenticated = 16,
}

impl GrpcCode {
    pub(crate) fn into_bitmask(self) -> GrpcCodeBitmask {
        match self {
            Self::Ok => GrpcCodeBitmask::OK,
            Self::Cancelled => GrpcCodeBitmask::CANCELLED,
            Self::Unknown => GrpcCodeBitmask::UNKNOWN,
            Self::InvalidArgument => GrpcCodeBitmask::INVALID_ARGUMENT,
            Self::DeadlineExceeded => GrpcCodeBitmask::DEADLINE_EXCEEDED,
            Self::NotFound => GrpcCodeBitmask::NOT_FOUND,
            Self::AlreadyExists => GrpcCodeBitmask::ALREADY_EXISTS,
            Self::PermissionDenied => GrpcCodeBitmask::PERMISSION_DENIED,
            Self::ResourceExhausted => GrpcCodeBitmask::RESOURCE_EXHAUSTED,
            Self::FailedPrecondition => GrpcCodeBitmask::FAILED_PRECONDITION,
            Self::Aborted => GrpcCodeBitmask::ABORTED,
            Self::OutOfRange => GrpcCodeBitmask::OUT_OF_RANGE,
            Self::Unimplemented => GrpcCodeBitmask::UNIMPLEMENTED,
            Self::Internal => GrpcCodeBitmask::INTERNAL,
            Self::Unavailable => GrpcCodeBitmask::UNAVAILABLE,
            Self::DataLoss => GrpcCodeBitmask::DATA_LOSS,
            Self::Unauthenticated => GrpcCodeBitmask::UNAUTHENTICATED,
        }
    }

    fn from_i32(code: i32) -> Option<GrpcCode> {
        match code {
            0 => Some(GrpcCode::Ok),
            1 => Some(GrpcCode::Cancelled),
            2 => Some(GrpcCode::Unknown),
            3 => Some(GrpcCode::InvalidArgument),
            4 => Some(GrpcCode::DeadlineExceeded),
            5 => Some(GrpcCode::NotFound),
            6 => Some(GrpcCode::AlreadyExists),
            7 => Some(GrpcCode::PermissionDenied),
            8 => Some(GrpcCode::ResourceExhausted),
            9 => Some(GrpcCode::FailedPrecondition),
            10 => Some(GrpcCode::Aborted),
            11 => Some(GrpcCode::OutOfRange),
            12 => Some(GrpcCode::Unimplemented),
            13 => Some(GrpcCode::Internal),
            14 => Some(GrpcCode::Unavailable),
            15 => Some(GrpcCode::DataLoss),
            16 => Some(GrpcCode::Unauthenticated),
            _ => None,
        }
    }
}

/// Converts an `i32` gRPC status code into a [`GrpcCode`].
///
/// Unrecognized codes (outside 0-16) map to [`GrpcCode::Unknown`].
impl From<i32> for GrpcCode {
    fn from(value: i32) -> Self {
        match value {
            0 => GrpcCode::Ok,
            1 => GrpcCode::Cancelled,
            2 => GrpcCode::Unknown,
            3 => GrpcCode::InvalidArgument,
            4 => GrpcCode::DeadlineExceeded,
            5 => GrpcCode::NotFound,
            6 => GrpcCode::AlreadyExists,
            7 => GrpcCode::PermissionDenied,
            8 => GrpcCode::ResourceExhausted,
            9 => GrpcCode::FailedPrecondition,
            10 => GrpcCode::Aborted,
            11 => GrpcCode::OutOfRange,
            12 => GrpcCode::Unimplemented,
            13 => GrpcCode::Internal,
            14 => GrpcCode::Unavailable,
            15 => GrpcCode::DataLoss,
            16 => GrpcCode::Unauthenticated,

            _ => GrpcCode::Unknown,
        }
    }
}

impl From<NonZeroI32> for GrpcCode {
    fn from(value: NonZeroI32) -> Self {
        GrpcCode::from(value.get())
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct GrpcCodeBitmask: u32 {
        const OK                  = 0b00000000000000001;
        const CANCELLED           = 0b00000000000000010;
        const UNKNOWN             = 0b00000000000000100;
        const INVALID_ARGUMENT    = 0b00000000000001000;
        const DEADLINE_EXCEEDED   = 0b00000000000010000;
        const NOT_FOUND           = 0b00000000000100000;
        const ALREADY_EXISTS      = 0b00000000001000000;
        const PERMISSION_DENIED   = 0b00000000010000000;
        const RESOURCE_EXHAUSTED  = 0b00000000100000000;
        const FAILED_PRECONDITION = 0b00000001000000000;
        const ABORTED             = 0b00000010000000000;
        const OUT_OF_RANGE        = 0b00000100000000000;
        const UNIMPLEMENTED       = 0b00001000000000000;
        const INTERNAL            = 0b00010000000000000;
        const UNAVAILABLE         = 0b00100000000000000;
        const DATA_LOSS           = 0b01000000000000000;
        const UNAUTHENTICATED     = 0b10000000000000000;
    }
}

impl From<GrpcCode> for GrpcCodeBitmask {
    fn from(code: GrpcCode) -> Self {
        match code {
            GrpcCode::Ok => GrpcCodeBitmask::OK,
            GrpcCode::Cancelled => GrpcCodeBitmask::CANCELLED,
            GrpcCode::Unknown => GrpcCodeBitmask::UNKNOWN,
            GrpcCode::InvalidArgument => GrpcCodeBitmask::INVALID_ARGUMENT,
            GrpcCode::DeadlineExceeded => GrpcCodeBitmask::DEADLINE_EXCEEDED,
            GrpcCode::NotFound => GrpcCodeBitmask::NOT_FOUND,
            GrpcCode::AlreadyExists => GrpcCodeBitmask::ALREADY_EXISTS,
            GrpcCode::PermissionDenied => GrpcCodeBitmask::PERMISSION_DENIED,
            GrpcCode::ResourceExhausted => GrpcCodeBitmask::RESOURCE_EXHAUSTED,
            GrpcCode::FailedPrecondition => GrpcCodeBitmask::FAILED_PRECONDITION,
            GrpcCode::Aborted => GrpcCodeBitmask::ABORTED,
            GrpcCode::OutOfRange => GrpcCodeBitmask::OUT_OF_RANGE,
            GrpcCode::Unimplemented => GrpcCodeBitmask::UNIMPLEMENTED,
            GrpcCode::Internal => GrpcCodeBitmask::INTERNAL,
            GrpcCode::Unavailable => GrpcCodeBitmask::UNAVAILABLE,
            GrpcCode::DataLoss => GrpcCodeBitmask::DATA_LOSS,
            GrpcCode::Unauthenticated => GrpcCodeBitmask::UNAUTHENTICATED,
        }
    }
}

/// Response classifier for gRPC responses.
///
/// gRPC doesn't use normal HTTP statuses for indicating success or failure but instead a special
/// header that might appear in a trailer.
///
/// Responses are considered successful if
///
/// - `grpc-status` header value contains a success value.
/// - `grpc-status` header is missing.
/// - `grpc-status` header value isn't a valid `String`.
/// - `grpc-status` header value can't parsed into an `i32`.
///
/// All others are considered failures.
#[derive(Debug, Clone)]
pub struct GrpcErrorsAsFailures {
    success_codes: GrpcCodeBitmask,
}

impl Default for GrpcErrorsAsFailures {
    fn default() -> Self {
        Self::new()
    }
}

impl GrpcErrorsAsFailures {
    /// Create a new [`GrpcErrorsAsFailures`].
    pub fn new() -> Self {
        Self {
            success_codes: GrpcCodeBitmask::OK,
        }
    }

    /// Change which gRPC codes are considered success.
    ///
    /// Defaults to only considering `Ok` as success.
    ///
    /// `Ok` will always be considered a success.
    ///
    /// # Example
    ///
    /// Servers might not want to consider `Invalid Argument` or `Not Found` as failures since
    /// thats likely the clients fault:
    ///
    /// ```rust
    /// use tower_http::classify::{GrpcErrorsAsFailures, GrpcCode};
    ///
    /// let classifier = GrpcErrorsAsFailures::new()
    ///     .with_success(GrpcCode::InvalidArgument)
    ///     .with_success(GrpcCode::NotFound);
    /// ```
    pub fn with_success(mut self, code: GrpcCode) -> Self {
        self.success_codes |= code.into_bitmask();
        self
    }

    /// Returns a [`MakeClassifier`](super::MakeClassifier) that produces `GrpcErrorsAsFailures`.
    ///
    /// This is a convenience function that simply calls `SharedClassifier::new`.
    pub fn make_classifier() -> SharedClassifier<Self> {
        SharedClassifier::new(Self::new())
    }
}

impl ClassifyResponse for GrpcErrorsAsFailures {
    type FailureClass = GrpcFailureClass;
    type ClassifyEos = GrpcEosErrorsAsFailures;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
        match classify_grpc_metadata(res.headers(), self.success_codes) {
            ParsedGrpcStatus::Success | ParsedGrpcStatus::HeaderNotGrpcCode => {
                ClassifiedResponse::Ready(Ok(()))
            }
            ParsedGrpcStatus::NonSuccess(status) => {
                ClassifiedResponse::Ready(Err(GrpcFailureClass::Status(status)))
            }
            ParsedGrpcStatus::GrpcStatusHeaderMissing => {
                ClassifiedResponse::RequiresEos(GrpcEosErrorsAsFailures {
                    success_codes: self.success_codes,
                })
            }
        }
    }

    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static,
    {
        GrpcFailureClass::Error(error.to_string())
    }
}

/// The [`ClassifyEos`] for [`GrpcErrorsAsFailures`].
#[derive(Debug, Clone)]
pub struct GrpcEosErrorsAsFailures {
    success_codes: GrpcCodeBitmask,
}

impl ClassifyEos for GrpcEosErrorsAsFailures {
    type FailureClass = GrpcFailureClass;

    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass> {
        if let Some(trailers) = trailers {
            match classify_grpc_metadata(trailers, self.success_codes) {
                ParsedGrpcStatus::Success
                | ParsedGrpcStatus::GrpcStatusHeaderMissing
                | ParsedGrpcStatus::HeaderNotGrpcCode => Ok(()),
                ParsedGrpcStatus::NonSuccess(status) => Err(GrpcFailureClass::Status(status)),
            }
        } else {
            Ok(())
        }
    }

    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static,
    {
        GrpcFailureClass::Error(error.to_string())
    }
}

/// The failure class for [`GrpcErrorsAsFailures`].
#[derive(Debug)]
#[non_exhaustive]
pub enum GrpcFailureClass {
    /// A gRPC response was classified as a failure with the corresponding status.
    Status(GrpcStatus),
    /// A gRPC response was classified as an error with the corresponding error description.
    Error(String),
}

impl fmt::Display for GrpcFailureClass {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Status(status) => {
                write!(f, "Status: {}", status)
            }
            Self::Error(error) => write!(f, "Error: {}", error),
        }
    }
}

impl std::error::Error for GrpcFailureClass {}

pub(crate) fn classify_grpc_metadata(
    headers: &HeaderMap,
    success_codes: GrpcCodeBitmask,
) -> ParsedGrpcStatus {
    macro_rules! or_else {
        ($expr:expr, $other:ident) => {
            if let Some(value) = $expr {
                value
            } else {
                return ParsedGrpcStatus::$other;
            }
        };
    }

    let code_header = or_else!(headers.get("grpc-status"), GrpcStatusHeaderMissing);
    let code_value: i32 = or_else!(
        code_header.to_str().ok().and_then(|s| s.parse().ok()),
        HeaderNotGrpcCode
    );
    let grpc_code = GrpcCode::from_i32(code_value);

    if let Some(code) = grpc_code {
        if success_codes.contains(GrpcCodeBitmask::from(code)) {
            return ParsedGrpcStatus::Success;
        }
    }

    let message = headers.get("grpc-message").map(|header| {
        percent_decode(header.as_bytes())
            .decode_utf8_lossy()
            .into_owned()
    });

    ParsedGrpcStatus::NonSuccess(GrpcStatus {
        code: grpc_code,
        code_raw: code_value,
        message,
    })
}

/// A gRPC status extracted from response headers/trailers.
#[derive(Debug, PartialEq, Eq)]
pub struct GrpcStatus {
    code: Option<GrpcCode>,
    code_raw: i32,
    message: Option<String>,
}

impl GrpcStatus {
    /// Returns the status code as a [`GrpcCode`], or `None` if the code is not recognized.
    pub fn code(&self) -> Option<GrpcCode> {
        self.code
    }

    /// Returns the raw integer status code.
    pub fn code_raw(&self) -> i32 {
        self.code_raw
    }

    /// Returns the percent-decoded gRPC error message, if present.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Display for GrpcStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "{:?}", code)?,
            None => write!(f, "Code({})", self.code_raw)?,
        }
        if let Some(message) = self.message.as_ref() {
            write!(f, ": {}", message)?;
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParsedGrpcStatus {
    Success,
    NonSuccess(GrpcStatus),
    GrpcStatusHeaderMissing,
    // this is treated as `Success` but kept separate for clarity
    HeaderNotGrpcCode,
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! classify_grpc_metadata_test {
        (
            name: $name:ident,
            status: $status:expr,
            success_flags: $success_flags:expr,
            expected: $expected:expr,
        ) => {
            classify_grpc_metadata_test!(
                name: $name,
                status: $status,
                message: "",
                success_flags: $success_flags,
                expected: $expected,
            );
        };
        (
            name: $name:ident,
            status: $status:expr,
            message: $message:expr,
            success_flags: $success_flags:expr,
            expected: $expected:expr,
        ) => {
            #[test]
            fn $name() {
                let mut headers = HeaderMap::new();
                headers.insert("grpc-status", $status.parse().unwrap());
                if !$message.is_empty() {
                    headers.insert("grpc-message", $message.parse().unwrap());
                }
                let status = classify_grpc_metadata(&headers, $success_flags);
                assert_eq!(status, $expected);
            }
        };
    }

    classify_grpc_metadata_test! {
        name: basic_ok,
        status: "0",
        success_flags: GrpcCodeBitmask::OK,
        expected: ParsedGrpcStatus::Success,
    }

    classify_grpc_metadata_test! {
        name: basic_error,
        status: "1",
        success_flags: GrpcCodeBitmask::OK,
        expected: ParsedGrpcStatus::NonSuccess(GrpcStatus{
            code: Some(GrpcCode::Cancelled),
            code_raw: 1,
            message: None,
        }),
    }

    classify_grpc_metadata_test! {
        name: two_success_codes_first_matches,
        status: "0",
        success_flags: GrpcCodeBitmask::OK | GrpcCodeBitmask::INVALID_ARGUMENT,
        expected: ParsedGrpcStatus::Success,
    }

    classify_grpc_metadata_test! {
        name: two_success_codes_second_matches,
        status: "3",
        success_flags: GrpcCodeBitmask::OK | GrpcCodeBitmask::INVALID_ARGUMENT,
        expected: ParsedGrpcStatus::Success,
    }

    classify_grpc_metadata_test! {
        name: two_success_codes_none_matches,
        status: "16",
        message: "mock message",
        success_flags: GrpcCodeBitmask::OK | GrpcCodeBitmask::INVALID_ARGUMENT,
        expected: ParsedGrpcStatus::NonSuccess(GrpcStatus{
            code: Some(GrpcCode::Unauthenticated),
            code_raw: 16,
            message: Some("mock message".to_string()),
        }),
    }

    classify_grpc_metadata_test! {
        name: percent_encoded_message,
        status: "2",
        message: "hello%20world",
        success_flags: GrpcCodeBitmask::OK,
        expected: ParsedGrpcStatus::NonSuccess(GrpcStatus{
            code: Some(GrpcCode::Unknown),
            code_raw: 2,
            message: Some("hello world".to_string()),
        }),
    }

    classify_grpc_metadata_test! {
        name: invalid_percent_encoding,
        status: "13",
        message: "bad%2Gencode",
        success_flags: GrpcCodeBitmask::OK,
        expected: ParsedGrpcStatus::NonSuccess(GrpcStatus{
            code: Some(GrpcCode::Internal),
            code_raw: 13,
            message: Some("bad%2Gencode".to_string()),
        }),
    }

    classify_grpc_metadata_test! {
        name: empty_grpc_message,
        status: "5",
        message: "",
        success_flags: GrpcCodeBitmask::OK,
        expected: ParsedGrpcStatus::NonSuccess(GrpcStatus{
            code: Some(GrpcCode::NotFound),
            code_raw: 5,
            message: None,
        }),
    }

    classify_grpc_metadata_test! {
        name: unknown_status_code_above_16,
        status: "99",
        message: "custom error",
        success_flags: GrpcCodeBitmask::OK,
        expected: ParsedGrpcStatus::NonSuccess(GrpcStatus{
            code: None,
            code_raw: 99,
            message: Some("custom error".to_string()),
        }),
    }

    #[test]
    fn invalid_utf8_after_percent_decode() {
        let mut headers = HeaderMap::new();
        headers.insert("grpc-status", "2".parse().unwrap());
        // %80 is an invalid UTF-8 start byte; lossy decode replaces it with U+FFFD
        headers.insert("grpc-message", "bad%80byte".parse().unwrap());
        let status = classify_grpc_metadata(&headers, GrpcCodeBitmask::OK);
        assert_eq!(
            status,
            ParsedGrpcStatus::NonSuccess(GrpcStatus {
                code: Some(GrpcCode::Unknown),
                code_raw: 2,
                message: Some("bad\u{FFFD}byte".to_string()),
            })
        );
    }

    #[test]
    fn valid_utf8_percent_encoded() {
        let mut headers = HeaderMap::new();
        headers.insert("grpc-status", "3".parse().unwrap());
        // %C3%A9 is the percent-encoded form of 'é' (U+00E9) in UTF-8
        headers.insert("grpc-message", "caf%C3%A9".parse().unwrap());
        let status = classify_grpc_metadata(&headers, GrpcCodeBitmask::OK);
        assert_eq!(
            status,
            ParsedGrpcStatus::NonSuccess(GrpcStatus {
                code: Some(GrpcCode::InvalidArgument),
                code_raw: 3,
                message: Some("café".to_string()),
            })
        );
    }

    #[test]
    fn grpc_ok_classified_as_success() {
        use http::Response;

        let res = Response::builder()
            .header("grpc-status", "0")
            .body(())
            .unwrap();

        let classifier = GrpcErrorsAsFailures::new();
        let result = classifier.classify_response(&res);
        assert!(matches!(result, ClassifiedResponse::Ready(Ok(()))));
    }

    #[test]
    fn grpc_code_from_i32_known_codes() {
        assert!(matches!(GrpcCode::from(0), GrpcCode::Ok));
        assert!(matches!(GrpcCode::from(1), GrpcCode::Cancelled));
        assert!(matches!(GrpcCode::from(4), GrpcCode::DeadlineExceeded));
        assert!(matches!(GrpcCode::from(13), GrpcCode::Internal));
        assert!(matches!(GrpcCode::from(16), GrpcCode::Unauthenticated));
    }

    #[test]
    fn grpc_code_from_i32_unknown_codes() {
        assert!(matches!(GrpcCode::from(17), GrpcCode::Unknown));
        assert!(matches!(GrpcCode::from(-1), GrpcCode::Unknown));
        assert!(matches!(GrpcCode::from(9999), GrpcCode::Unknown));
    }

    #[test]
    fn grpc_code_from_non_zero_i32() {
        let code = NonZeroI32::new(7).unwrap();
        assert!(matches!(GrpcCode::from(code), GrpcCode::PermissionDenied));

        let code = NonZeroI32::new(99).unwrap();
        assert!(matches!(GrpcCode::from(code), GrpcCode::Unknown));
    }
}
