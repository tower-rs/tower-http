use std::time::Duration;

pub trait OnBodyChunk<B> {
    fn on_body_chunk(&mut self, chunk: &B, latency: Duration);
}

impl<B, F> OnBodyChunk<B> for F
where
    F: FnMut(&B, Duration),
{
    fn on_body_chunk(&mut self, chunk: &B, latency: Duration) {
        self(chunk, latency)
    }
}

#[derive(Debug, Default, Clone)]
pub struct DefaultOnBodyChunk {
    _priv: (),
}

impl DefaultOnBodyChunk {
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl<B> OnBodyChunk<B> for DefaultOnBodyChunk {
    #[inline]
    fn on_body_chunk(&mut self, _: &B, _: Duration) {}
}
