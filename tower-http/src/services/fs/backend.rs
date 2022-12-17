use futures_util::future::BoxFuture;
use std::{future::Future, io, path::Path, time::SystemTime};
use tokio::io::{AsyncRead, AsyncSeek};

// TODO(david): try and rewrite this using async-trait to see if that matters much
// currently this requires GATs which is maybe pushing tower-http's MSRV a bit?
//
// async-trait is unfortunate because it requires syn+quote and requiring allocations
// futures is unfortunate if the data is in the binary, as for rust-embed

// TODO(david): implement a backend using rust-embed to prove that its possible

pub trait Backend: Clone + Send + Sync + 'static {
    type File: File<Metadata = Self::Metadata>;
    type Metadata: Metadata;

    type OpenFuture: Future<Output = io::Result<Self::File>> + Send;
    type MetadataFuture: Future<Output = io::Result<Self::Metadata>> + Send;

    fn open<A>(&self, path: A) -> Self::OpenFuture
    where
        A: AsRef<Path>;

    fn metadata<A>(&self, path: A) -> Self::MetadataFuture
    where
        A: AsRef<Path>;
}

pub trait Metadata: Send + 'static {
    fn is_dir(&self) -> bool;

    fn modified(&self) -> io::Result<SystemTime>;

    fn len(&self) -> u64;
}

pub trait File: AsyncRead + AsyncSeek + Unpin + Send + Sync {
    type Metadata: Metadata;
    type MetadataFuture<'a>: Future<Output = io::Result<Self::Metadata>> + Send
    where
        Self: 'a;

    fn metadata(&self) -> Self::MetadataFuture<'_>;
}

#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub struct TokioBackend;

impl Backend for TokioBackend {
    type File = tokio::fs::File;
    type Metadata = std::fs::Metadata;

    type OpenFuture = BoxFuture<'static, io::Result<Self::File>>;
    type MetadataFuture = BoxFuture<'static, io::Result<Self::Metadata>>;

    fn open<A>(&self, path: A) -> Self::OpenFuture
    where
        A: AsRef<Path>,
    {
        let path = path.as_ref().to_owned();
        Box::pin(tokio::fs::File::open(path))
    }

    fn metadata<A>(&self, path: A) -> Self::MetadataFuture
    where
        A: AsRef<Path>,
    {
        let path = path.as_ref().to_owned();
        Box::pin(tokio::fs::metadata(path))
    }
}

impl File for tokio::fs::File {
    type Metadata = std::fs::Metadata;
    type MetadataFuture<'a> = BoxFuture<'a, io::Result<Self::Metadata>>;

    fn metadata(&self) -> Self::MetadataFuture<'_> {
        Box::pin(self.metadata())
    }
}

impl Metadata for std::fs::Metadata {
    #[inline]
    fn is_dir(&self) -> bool {
        self.is_dir()
    }

    #[inline]
    fn modified(&self) -> io::Result<SystemTime> {
        self.modified()
    }

    #[inline]
    fn len(&self) -> u64 {
        self.len()
    }
}
