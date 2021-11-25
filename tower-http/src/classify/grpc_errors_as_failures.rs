use super::{ClassifiedResponse, ClassifyEos, ClassifyResponse, SharedClassifier};
use bitflags::bitflags;
use http::{HeaderMap, Response};
use std::{fmt, num::NonZeroI32};

bitflags! {
    /// Bitmask used to specify which gRPC status codes are considered successes.
    ///
    /// Used in [`GrpcErrorsAsFailures::success_codes`].
    pub struct GrpcCodeBitmask: u32 {
        /// The operation completed successfully.
        const OK                  = 0b00000000000000001;
        /// The operation was cancelled.
        const CANCELLED           = 0b00000000000000010;
        /// Unknown error.
        const UNKNOWN             = 0b00000000000000100;
        /// Client specified an invalid argument.
        const INVALID_ARGUMENT    = 0b00000000000001000;
        /// Deadline expired before operation could complete.
        const DEADLINE_EXCEEDED   = 0b00000000000010000;
        /// Some requested entity was not found.
        const NOT_FOUND           = 0b00000000000100000;
        /// Some entity that we attempted to create already exists.
        const ALREADY_EXISTS      = 0b00000000001000000;
        /// The caller does not have permission to execute the specified operation.
        const PERMISSION_DENIED   = 0b00000000010000000;
        /// Some resource has been exhausted.
        const RESOURCE_EXHAUSTED  = 0b00000000100000000;
        /// The system is not in a state required for the operation's execution.
        const FAILED_PRECONDITION = 0b00000001000000000;
        /// The operation was aborted.
        const ABORTED             = 0b00000010000000000;
        /// Operation was attempted past the valid range.
        const OUT_OF_RANGE        = 0b00000100000000000;
        /// Operation is not implemented or not supported.
        const UNIMPLEMENTED       = 0b00001000000000000;
        /// Internal error.
        const INTERNAL            = 0b00010000000000000;
        /// The service is currently unavailable.
        const UNAVAILABLE         = 0b00100000000000000;
        /// Unrecoverable data loss or corruption.
        const DATA_LOSS           = 0b01000000000000000;
        /// The request does not have valid authentication credentials
        const UNAUTHENTICATED     = 0b10000000000000000;
    }
}

impl GrpcCodeBitmask {
    fn try_from_u32(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::OK),
            1 => Some(Self::CANCELLED),
            2 => Some(Self::UNKNOWN),
            3 => Some(Self::INVALID_ARGUMENT),
            4 => Some(Self::DEADLINE_EXCEEDED),
            5 => Some(Self::NOT_FOUND),
            6 => Some(Self::ALREADY_EXISTS),
            7 => Some(Self::PERMISSION_DENIED),
            8 => Some(Self::RESOURCE_EXHAUSTED),
            9 => Some(Self::FAILED_PRECONDITION),
            10 => Some(Self::ABORTED),
            11 => Some(Self::OUT_OF_RANGE),
            12 => Some(Self::UNIMPLEMENTED),
            13 => Some(Self::INTERNAL),
            14 => Some(Self::UNAVAILABLE),
            15 => Some(Self::DATA_LOSS),
            16 => Some(Self::UNAUTHENTICATED),
            _ => None,
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
/// - `grpc-status` header value matches [`GrpcErrorsAsFailures::success_codes`] (only `Ok` by
/// default).
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
    /// Calling this method multiple times overrides previous calls.
    ///
    /// Use `|` to combine multiple codes, ie `GrpcCodeBitmask::INVALID_ARGUMENT | GrpcCodeBitmask::NOT_FOUND`.
    ///
    /// `Ok` will always be considered a success.
    ///
    /// # Example
    ///
    /// Servers might not want to consider `Invalid Argument` or `Not Found` as failures since
    /// thats likely the clients fault:
    ///
    /// ```rust
    /// use tower_http::classify::{GrpcErrorsAsFailures, GrpcCodeBitmask};
    ///
    /// let classifier = GrpcErrorsAsFailures::new().success_codes(
    ///     GrpcCodeBitmask::INVALID_ARGUMENT | GrpcCodeBitmask::NOT_FOUND,
    /// );
    /// ```
    pub fn success_codes(mut self, codes: GrpcCodeBitmask) -> Self {
        self.success_codes = GrpcCodeBitmask::OK | codes;
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
            ParsedGrpcStatus::Success
            | ParsedGrpcStatus::HeaderNotString
            | ParsedGrpcStatus::HeaderNotInt => ClassifiedResponse::Ready(Ok(())),
            ParsedGrpcStatus::NonSuccess(status) => {
                ClassifiedResponse::Ready(Err(GrpcFailureClass::Code(status)))
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
                | ParsedGrpcStatus::HeaderNotString
                | ParsedGrpcStatus::HeaderNotInt => Ok(()),
                ParsedGrpcStatus::NonSuccess(status) => Err(GrpcFailureClass::Code(status)),
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
pub enum GrpcFailureClass {
    /// A gRPC response was classified as a failure with the corresponding status.
    Code(std::num::NonZeroI32),
    /// A gRPC response was classified as an error with the corresponding error description.
    Error(String),
}

impl fmt::Display for GrpcFailureClass {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Code(code) => write!(f, "Code: {}", code),
            Self::Error(error) => write!(f, "Error: {}", error),
        }
    }
}

#[allow(clippy::if_let_some_result)]
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

    let status = or_else!(headers.get("grpc-status"), GrpcStatusHeaderMissing);
    let status = or_else!(status.to_str().ok(), HeaderNotString);
    let status = or_else!(status.parse::<i32>().ok(), HeaderNotInt);

    if GrpcCodeBitmask::try_from_u32(status as _)
        .filter(|code| success_codes.contains(*code))
        .is_some()
    {
        ParsedGrpcStatus::Success
    } else {
        ParsedGrpcStatus::NonSuccess(NonZeroI32::new(status).unwrap())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParsedGrpcStatus {
    Success,
    NonSuccess(NonZeroI32),
    GrpcStatusHeaderMissing,
    // these two are treated as `Success` but kept separate for clarity
    HeaderNotString,
    HeaderNotInt,
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
            #[test]
            fn $name() {
                let mut headers = HeaderMap::new();
                headers.insert("grpc-status", $status.parse().unwrap());
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
        expected: ParsedGrpcStatus::NonSuccess(NonZeroI32::new(1).unwrap()),
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
        success_flags: GrpcCodeBitmask::OK | GrpcCodeBitmask::INVALID_ARGUMENT,
        expected: ParsedGrpcStatus::NonSuccess(NonZeroI32::new(16).unwrap()),
    }
}
