use super::{
    headers::{ETag, IfMatch, IfModifiedSince, IfNoneMatch, IfUnmodifiedSince, LastModified},
    ServeVariant,
};
use crate::content_encoding::{Encoding, QValue};
use bytes::Bytes;
use http::{header, HeaderValue, Method, Request, Uri};
use http_body_util::Empty;
use http_range_header::RangeUnsatisfiableError;
use std::{
    ffi::OsStr,
    fs::Metadata,
    io::{self, ErrorKind, SeekFrom},
    ops::RangeInclusive,
    path::{Path, PathBuf},
};
use tokio::{fs::File, io::AsyncSeekExt};

pub(super) enum OpenFileOutput {
    FileOpened(Box<FileOpened>),
    Redirect {
        location: HeaderValue,
    },
    FileNotFound,
    PreconditionFailed,
    NotModified {
        etag: Option<ETag>,
        last_modified: Option<LastModified>,
    },
    InvalidRedirectUri,
    InvalidFilename,
}

pub(super) struct FileOpened {
    pub(super) extent: FileRequestExtent,
    pub(super) chunk_size: usize,
    pub(super) mime_header_value: HeaderValue,
    pub(super) maybe_encoding: Option<Encoding>,
    pub(super) maybe_range: Option<Result<Vec<RangeInclusive<u64>>, RangeUnsatisfiableError>>,
    pub(super) last_modified: Option<LastModified>,
    pub(super) precompression_configured: bool,
    pub(super) etag: Option<ETag>,
}

pub(super) enum FileRequestExtent {
    Full(File, Metadata),
    Head(Metadata),
}

pub(super) async fn open_file(
    variant: ServeVariant,
    mut path_to_file: PathBuf,
    req: Request<Empty<Bytes>>,
    negotiated_encodings: Vec<(Encoding, QValue)>,
    range_header: Option<String>,
    buf_chunk_size: usize,
    precompression_configured: bool,
) -> io::Result<OpenFileOutput> {
    let preconditions = Preconditions {
        if_match: req
            .headers()
            .get(header::IF_MATCH)
            .and_then(IfMatch::from_header_value),
        if_unmodified_since: req
            .headers()
            .get(header::IF_UNMODIFIED_SINCE)
            .and_then(IfUnmodifiedSince::from_header_value),
        if_none_match: req
            .headers()
            .get(header::IF_NONE_MATCH)
            .and_then(IfNoneMatch::from_header_value),
        if_modified_since: req
            .headers()
            .get(header::IF_MODIFIED_SINCE)
            .and_then(IfModifiedSince::from_header_value),
    };

    let mime = match variant {
        ServeVariant::Directory {
            append_index_html_on_directories,
            html_as_default_extension,
        } => {
            // Might already at this point know a redirect or not found result should be
            // returned which corresponds to a Some(output). Otherwise the path might be
            // modified and proceed to the open file/metadata future.
            if let Some(output) = maybe_redirect_or_append_path(
                &mut path_to_file,
                req.uri(),
                append_index_html_on_directories,
                html_as_default_extension,
            )
            .await
            {
                return Ok(output);
            }

            mime_guess::from_path(&path_to_file)
                .first_raw()
                .map(HeaderValue::from_static)
                .unwrap_or_else(|| {
                    HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
                })
        }

        ServeVariant::SingleFile { mime } => mime,
    };

    if req.method() == Method::HEAD {
        #[cfg(feature = "tracing")]
        let _path_str = path_to_file.display().to_string();
        let (meta, maybe_encoding) =
            file_metadata_with_fallback(path_to_file, negotiated_encodings).await?;

        let last_modified = meta.modified().ok().map(LastModified::from);
        let etag = meta
            .modified()
            .ok()
            .and_then(|mtime| ETag::from_metadata(meta.len(), mtime));

        #[cfg(feature = "tracing")]
        if etag.is_none() {
            rate_limited!(
                std::time::Duration::from_secs(60),
                tracing::warn!(path = %_path_str, "ETag generation failed (mtime unavailable or pre-epoch)")
            );
        }

        if let Some(output) = preconditions.check(etag.as_ref(), last_modified.as_ref()) {
            return Ok(output);
        }

        let maybe_range = try_parse_range(range_header.as_deref(), meta.len());

        Ok(OpenFileOutput::FileOpened(Box::new(FileOpened {
            extent: FileRequestExtent::Head(meta),
            chunk_size: buf_chunk_size,
            mime_header_value: mime,
            maybe_encoding,
            maybe_range,
            last_modified,
            precompression_configured,
            etag,
        })))
    } else {
        #[cfg(feature = "tracing")]
        let _path_str = path_to_file.display().to_string();
        let (mut file, maybe_encoding) =
            match open_file_with_fallback(path_to_file, negotiated_encodings).await {
                Ok(result) => result,

                Err(err) if is_invalid_filename_error(&err) => {
                    return Ok(OpenFileOutput::InvalidFilename)
                }
                Err(err) => return Err(err),
            };

        let meta = file.metadata().await?;

        let last_modified = meta.modified().ok().map(LastModified::from);
        let etag = meta
            .modified()
            .ok()
            .and_then(|mtime| ETag::from_metadata(meta.len(), mtime));

        #[cfg(feature = "tracing")]
        if etag.is_none() {
            rate_limited!(
                std::time::Duration::from_secs(60),
                tracing::warn!(path = %_path_str, "ETag generation failed (mtime unavailable or pre-epoch)")
            );
        }

        if let Some(output) = preconditions.check(etag.as_ref(), last_modified.as_ref()) {
            return Ok(output);
        }

        let maybe_range = try_parse_range(range_header.as_deref(), meta.len());
        if let Some(Ok(ranges)) = maybe_range.as_ref() {
            // if there is any other amount of ranges than 1 we'll return an
            // unsatisfiable later as there isn't yet support for multipart ranges
            if ranges.len() == 1 {
                file.seek(SeekFrom::Start(*ranges[0].start())).await?;
            }
        }

        Ok(OpenFileOutput::FileOpened(Box::new(FileOpened {
            extent: FileRequestExtent::Full(file, meta),
            chunk_size: buf_chunk_size,
            mime_header_value: mime,
            maybe_encoding,
            maybe_range,
            last_modified,
            precompression_configured,
            etag,
        })))
    }
}

fn is_invalid_filename_error(err: &io::Error) -> bool {
    // Only applies to NULL bytes
    if err.kind() == ErrorKind::InvalidInput {
        return true;
    }

    // FIXME: Remove when MSRV >= 1.87.
    // `io::ErrorKind::InvalidFilename` is stabilized in v1.87
    #[cfg(windows)]
    if let Some(raw_err) = err.raw_os_error() {
        // https://github.com/rust-lang/rust/blob/70e2b4a4d197f154bed0eb3dcb5cac6a948ff3a3/library/std/src/sys/pal/windows/mod.rs
        // Lines 81 and 115
        if (raw_err == 123) || (raw_err == 161) || (raw_err == 206) {
            return true;
        }
    }

    false
}

/// Precondition headers parsed from the request.
struct Preconditions {
    if_match: Option<IfMatch>,
    if_unmodified_since: Option<IfUnmodifiedSince>,
    if_none_match: Option<IfNoneMatch>,
    if_modified_since: Option<IfModifiedSince>,
}

impl Preconditions {
    /// Evaluate preconditions per [RFC 9110 §13.2.2](https://www.rfc-editor.org/rfc/rfc9110#section-13.2.2).
    ///
    /// Precedence order:
    /// 1. If-Match (strong comparison) → 412 on failure
    /// 2. If-Unmodified-Since (only if If-Match absent) → 412 on failure
    /// 3. If-None-Match (weak comparison) → 304 on failure (for GET/HEAD)
    /// 4. If-Modified-Since (only if If-None-Match absent) → 304 on failure
    fn check(
        self,
        etag: Option<&ETag>,
        last_modified: Option<&LastModified>,
    ) -> Option<OpenFileOutput> {
        // Step 1: If-Match
        if let Some(if_match) = self.if_match {
            // RFC 9110 §13.1.1: "If the field value is '*', the condition is FALSE
            // if the origin server does not have a current representation."
            // No ETag means no current representation → fail.
            let passes = etag
                .map(|etag| if_match.precondition_passes(etag))
                .unwrap_or(false);
            if !passes {
                return Some(OpenFileOutput::PreconditionFailed);
            }
        } else {
            // Step 2: If-Unmodified-Since (only when If-Match is absent)
            // RFC 9110 §13.1.4: "MUST ignore if the resource does not have a
            // modification date available."
            if let Some(since) = self.if_unmodified_since {
                let passes = last_modified
                    .map(|lm| since.precondition_passes(lm))
                    .unwrap_or(true);
                if !passes {
                    return Some(OpenFileOutput::PreconditionFailed);
                }
            }
        }

        // Step 3: If-None-Match
        if let Some(if_none_match) = self.if_none_match {
            // No ETag available → condition is vacuously true (passes), serve normally.
            let passes = etag
                .map(|etag| if_none_match.precondition_passes(etag))
                .unwrap_or(true);
            if !passes {
                return Some(OpenFileOutput::NotModified {
                    etag: etag.cloned(),
                    last_modified: last_modified.map(|lm| LastModified(lm.0)),
                });
            }
        } else {
            // Step 4: If-Modified-Since (only when If-None-Match is absent)
            // No Last-Modified → treat as modified (serve normally).
            if let Some(since) = self.if_modified_since {
                let unmodified = last_modified
                    .map(|lm| !since.is_modified(lm))
                    .unwrap_or(false);
                if unmodified {
                    return Some(OpenFileOutput::NotModified {
                        etag: etag.cloned(),
                        last_modified: last_modified.map(|lm| LastModified(lm.0)),
                    });
                }
            }
        }

        None
    }
}

// Returns the preferred_encoding encoding and modifies the path extension
// to the corresponding file extension for the encoding.
fn preferred_encoding(
    path: &mut PathBuf,
    negotiated_encoding: &[(Encoding, QValue)],
) -> Option<Encoding> {
    let preferred_encoding = Encoding::preferred_encoding(negotiated_encoding.iter().copied());

    if let Some(file_extension) =
        preferred_encoding.and_then(|encoding| encoding.to_file_extension())
    {
        let new_file_name = path
            .file_name()
            .map(|file_name| {
                let mut os_string = file_name.to_os_string();
                os_string.push(file_extension);
                os_string
            })
            .unwrap_or_else(|| file_extension.to_os_string());

        path.set_file_name(new_file_name);
    }

    preferred_encoding
}

// Attempts to open the file with any of the possible negotiated_encodings in the
// preferred order. If none of the negotiated_encodings have a corresponding precompressed
// file the uncompressed file is used as a fallback.
async fn open_file_with_fallback(
    mut path: PathBuf,
    mut negotiated_encoding: Vec<(Encoding, QValue)>,
) -> io::Result<(File, Option<Encoding>)> {
    let (file, encoding) = loop {
        // Get the preferred encoding among the negotiated ones.
        let encoding = preferred_encoding(&mut path, &negotiated_encoding);
        match (File::open(&path).await, encoding) {
            (Ok(file), maybe_encoding) => break (file, maybe_encoding),
            (Err(err), Some(encoding))
                if err.kind() == io::ErrorKind::NotFound && encoding != Encoding::Identity =>
            {
                // Remove the extension corresponding to a precompressed file (.gz, .br, .zz)
                // to reset the path before the next iteration.
                path.set_extension(OsStr::new(""));
                // Remove the encoding from the negotiated_encodings since the file doesn't exist
                negotiated_encoding
                    .retain(|(negotiated_encoding, _)| *negotiated_encoding != encoding);
            }
            (Err(err), _) => return Err(err),
        }
    };
    Ok((file, encoding))
}

// Attempts to get the file metadata with any of the possible negotiated_encodings in the
// preferred order. If none of the negotiated_encodings have a corresponding precompressed
// file the uncompressed file is used as a fallback.
async fn file_metadata_with_fallback(
    mut path: PathBuf,
    mut negotiated_encoding: Vec<(Encoding, QValue)>,
) -> io::Result<(Metadata, Option<Encoding>)> {
    let (file, encoding) = loop {
        // Get the preferred encoding among the negotiated ones.
        let encoding = preferred_encoding(&mut path, &negotiated_encoding);
        match (tokio::fs::metadata(&path).await, encoding) {
            (Ok(file), maybe_encoding) => break (file, maybe_encoding),
            (Err(err), Some(encoding))
                if err.kind() == io::ErrorKind::NotFound && encoding != Encoding::Identity =>
            {
                // Remove the extension corresponding to a precompressed file (.gz, .br, .zz)
                // to reset the path before the next iteration.
                path.set_extension(OsStr::new(""));
                // Remove the encoding from the negotiated_encodings since the file doesn't exist
                negotiated_encoding
                    .retain(|(negotiated_encoding, _)| *negotiated_encoding != encoding);
            }
            (Err(err), _) => return Err(err),
        }
    };
    Ok((file, encoding))
}

async fn maybe_redirect_or_append_path(
    path_to_file: &mut PathBuf,
    uri: &Uri,
    append_index_html_on_directories: bool,
    html_as_default_extension: bool,
) -> Option<OpenFileOutput> {
    let uri_path = uri.path();

    let is_directory = is_dir(path_to_file).await;

    if uri_path.ends_with('/') && uri_path != "/" && is_directory != Some(true) {
        return Some(OpenFileOutput::FileNotFound);
    }

    // If the path has no extension and doesn't exist as a file, try appending .html
    if html_as_default_extension && is_directory.is_none() && path_to_file.extension().is_none() {
        path_to_file.set_extension("html");
        return None;
    }

    if is_directory != Some(true) {
        return None;
    }

    if !append_index_html_on_directories {
        return Some(OpenFileOutput::FileNotFound);
    }

    if uri_path.ends_with('/') {
        path_to_file.push("index.html");
        None
    } else {
        let uri = match append_slash_on_path(uri.clone()) {
            Ok(uri) => uri,
            Err(err) => return Some(err),
        };
        let location = HeaderValue::from_str(&uri.to_string()).unwrap();
        Some(OpenFileOutput::Redirect { location })
    }
}

fn try_parse_range(
    maybe_range_ref: Option<&str>,
    file_size: u64,
) -> Option<Result<Vec<RangeInclusive<u64>>, RangeUnsatisfiableError>> {
    maybe_range_ref.map(|header_value| {
        http_range_header::parse_range_header(header_value)
            .and_then(|first_pass| first_pass.validate(file_size))
    })
}

async fn is_dir(path_to_file: &Path) -> Option<bool> {
    tokio::fs::metadata(path_to_file)
        .await
        .ok()
        .map(|meta_data| meta_data.is_dir())
}

fn append_slash_on_path(uri: Uri) -> Result<Uri, OpenFileOutput> {
    let http::uri::Parts {
        scheme,
        authority,
        path_and_query,
        ..
    } = uri.into_parts();

    let mut uri_builder = Uri::builder();

    if let Some(scheme) = scheme {
        uri_builder = uri_builder.scheme(scheme);
    }

    if let Some(authority) = authority {
        uri_builder = uri_builder.authority(authority);
    }

    let uri_builder = if let Some(path_and_query) = path_and_query {
        if let Some(query) = path_and_query.query() {
            uri_builder.path_and_query(format!("{}/?{}", path_and_query.path(), query))
        } else {
            uri_builder.path_and_query(format!("{}/", path_and_query.path()))
        }
    } else {
        uri_builder.path_and_query("/")
    };

    uri_builder.build().map_err(|_err| {
        #[cfg(feature = "tracing")]
        tracing::error!(err = ?_err, "redirect uri failed to build");

        OpenFileOutput::InvalidRedirectUri
    })
}

#[test]
fn preferred_encoding_with_extension() {
    let mut path = PathBuf::from("hello.txt");
    preferred_encoding(&mut path, &[(Encoding::Gzip, QValue::one())]);
    assert_eq!(path, PathBuf::from("hello.txt.gz"));
}

#[test]
fn preferred_encoding_without_extension() {
    let mut path = PathBuf::from("hello");
    preferred_encoding(&mut path, &[(Encoding::Gzip, QValue::one())]);
    assert_eq!(path, PathBuf::from("hello.gz"));
}
