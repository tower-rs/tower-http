//! Pluggable backend trait for [`ServeDir`](super::ServeDir).
//!
//! The [`Backend`] trait abstracts file system operations so that `ServeDir` can serve
//! files from sources other than the local filesystem (e.g. rust-embed, include_dir, S3).

use std::{future::Future, io, path::PathBuf, pin::Pin, time::SystemTime};
use tokio::io::{AsyncRead, AsyncSeek};

/// Trait for file metadata.
///
/// This is the information `ServeDir` needs about a file or directory without opening it.
pub trait Metadata: Send + 'static {
    /// Returns `true` if this metadata refers to a directory.
    fn is_dir(&self) -> bool;

    /// Returns the last modification time, if available.
    fn modified(&self) -> io::Result<SystemTime>;

    /// Returns the size of the file in bytes.
    fn len(&self) -> u64;

    /// Returns `true` if the file is empty (zero bytes).
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Trait for an opened file.
///
/// Must support async reading and seeking (for HTTP range requests).
/// In-memory backends can use [`std::io::Cursor`] to satisfy the `AsyncSeek` requirement.
pub trait File: AsyncRead + AsyncSeek + Unpin + Send + Sync {
    /// The metadata type returned by this file.
    type Metadata: Metadata;

    /// Future returned by [`File::metadata`].
    type MetadataFuture<'a>: Future<Output = io::Result<Self::Metadata>> + Send
    where
        Self: 'a;

    /// Returns metadata for this opened file.
    fn metadata(&self) -> Self::MetadataFuture<'_>;
}

/// Trait abstracting filesystem operations for [`ServeDir`](super::ServeDir).
///
/// Implement this trait to serve files from non-filesystem sources.
/// The default implementation ([`TokioBackend`]) wraps `tokio::fs`.
pub trait Backend: Clone + Send + Sync + 'static {
    /// The file type returned by [`Backend::open`].
    type File: File<Metadata = Self::Metadata>;

    /// The metadata type returned by [`Backend::metadata`].
    type Metadata: Metadata;

    /// Future returned by [`Backend::open`].
    type OpenFuture: Future<Output = io::Result<Self::File>> + Send;

    /// Future returned by [`Backend::metadata`].
    type MetadataFuture: Future<Output = io::Result<Self::Metadata>> + Send;

    /// Open a file at the given path.
    fn open(&self, path: PathBuf) -> Self::OpenFuture;

    /// Retrieve metadata for the given path without opening the file.
    fn metadata(&self, path: PathBuf) -> Self::MetadataFuture;
}

/// Default [`Backend`] implementation using `tokio::fs`.
#[derive(Clone, Debug, Default)]
pub struct TokioBackend;

impl Backend for TokioBackend {
    type File = TokioFile;
    type Metadata = std::fs::Metadata;
    type OpenFuture = Pin<Box<dyn Future<Output = io::Result<TokioFile>> + Send>>;
    type MetadataFuture = Pin<Box<dyn Future<Output = io::Result<std::fs::Metadata>> + Send>>;

    fn open(&self, path: PathBuf) -> Self::OpenFuture {
        Box::pin(async move {
            let file = tokio::fs::File::open(&path).await?;
            Ok(TokioFile(file))
        })
    }

    fn metadata(&self, path: PathBuf) -> Self::MetadataFuture {
        Box::pin(async move { tokio::fs::metadata(&path).await })
    }
}

/// Wrapper around [`tokio::fs::File`] implementing the [`File`] trait.
#[derive(Debug)]
pub struct TokioFile(tokio::fs::File);

impl AsyncRead for TokioFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncSeek for TokioFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.0).start_seek(position)
    }

    fn poll_complete(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<u64>> {
        Pin::new(&mut self.0).poll_complete(cx)
    }
}

impl File for TokioFile {
    type Metadata = std::fs::Metadata;
    type MetadataFuture<'a> =
        Pin<Box<dyn Future<Output = io::Result<std::fs::Metadata>> + Send + 'a>>;

    fn metadata(&self) -> Self::MetadataFuture<'_> {
        Box::pin(async move { self.0.metadata().await })
    }
}

impl Metadata for std::fs::Metadata {
    fn is_dir(&self) -> bool {
        self.is_dir()
    }

    fn modified(&self) -> io::Result<SystemTime> {
        self.modified()
    }

    fn len(&self) -> u64 {
        self.len()
    }
}
