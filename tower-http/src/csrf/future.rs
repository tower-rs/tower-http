use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project_lite::pin_project;

pin_project! {
    /// Response future for [`Csrf`].
    ///
    /// [`Csrf`]: super::Csrf
    pub struct ResponseFuture<F>
    where
        F: Future,
    {
        #[pin]
        kind: Kind<F>,
    }
}

pin_project! {
    #[project = KindProj]
    enum Kind<F>
    where
        F: Future,
    {
        Future {
            #[pin]
            future: F,
        },
        Rejected {
            response: Option<F::Output>,
        },
    }
}

impl<F> ResponseFuture<F>
where
    F: Future,
{
    pub(super) fn future(future: F) -> Self {
        Self {
            kind: Kind::Future { future },
        }
    }

    pub(super) fn rejected(response: F::Output) -> Self {
        Self {
            kind: Kind::Rejected {
                response: Some(response),
            },
        }
    }
}

impl<F> Future for ResponseFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future { future } => future.poll(cx),
            KindProj::Rejected { response } => Poll::Ready(
                response
                    .take()
                    .expect("ResponseFuture polled after completion"),
            ),
        }
    }
}
