use super::DEFAULT_MESSAGE_LEVEL;
use http::Request;
use tracing::Level;

pub trait OnRequest<B> {
    fn on_request(&mut self, request: &Request<B>);
}

impl<B, F> OnRequest<B> for F
where
    F: FnMut(&Request<B>),
{
    fn on_request(&mut self, request: &Request<B>) {
        self(request)
    }
}

#[derive(Clone, Debug)]
pub struct DefaultOnRequest {
    level: Level,
}

impl Default for DefaultOnRequest {
    fn default() -> Self {
        Self {
            level: DEFAULT_MESSAGE_LEVEL,
        }
    }
}

impl DefaultOnRequest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }
}

impl<B> OnRequest<B> for DefaultOnRequest {
    fn on_request(&mut self, _request: &Request<B>) {
        match self.level {
            Level::ERROR => {
                tracing::event!(Level::ERROR, "started processing request");
            }
            Level::WARN => {
                tracing::event!(Level::WARN, "started processing request");
            }
            Level::INFO => {
                tracing::event!(Level::INFO, "started processing request");
            }
            Level::DEBUG => {
                tracing::event!(Level::DEBUG, "started processing request");
            }
            Level::TRACE => {
                tracing::event!(Level::TRACE, "started processing request");
            }
        }
    }
}
