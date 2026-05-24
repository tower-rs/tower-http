use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::{Response, StatusCode};
use pin_project_lite::pin_project;

use super::ProtectionError;

pin_project! {
    /// Response future for [`Csrf`].
    ///
    /// [`Csrf`]: super::Csrf
    pub struct ResponseFuture<F, ResBody> {
        #[pin]
        kind: Kind<F, ResBody>,
    }
}

pin_project! {
    #[project = KindProj]
    enum Kind<F, ResBody> {
        Future { #[pin] future: F },
        Rejected {
            error: Option<ProtectionError>,
            _body: PhantomData<ResBody>,
        },
    }
}

impl<F, ResBody> ResponseFuture<F, ResBody> {
    pub(super) fn future(future: F) -> Self {
        Self {
            kind: Kind::Future { future },
        }
    }

    pub(super) fn rejected(error: ProtectionError) -> Self {
        Self {
            kind: Kind::Rejected {
                error: Some(error),
                _body: PhantomData,
            },
        }
    }
}

impl<F, E, ResBody> Future for ResponseFuture<F, ResBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Default,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future { future } => future.poll(cx),
            KindProj::Rejected { error, _body } => {
                let error = error
                    .take()
                    .expect("ResponseFuture polled after completion");
                let mut response = Response::new(ResBody::default());

                *response.status_mut() = StatusCode::FORBIDDEN;
                response.extensions_mut().insert(error);

                Poll::Ready(Ok(response))
            }
        }
    }
}
