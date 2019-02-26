use Body;
use util::IntoBufStream;

pub trait BodyExt: Body {
    /// Wrap the `Body` so that it implements tokio_buf::BufStream directly.
    fn into_buf_stream(self) -> IntoBufStream<Self>
    where
        Self: Sized,
    {
        IntoBufStream::new(self)
    }
}

impl<T: Body> BodyExt for T {}
