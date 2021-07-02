use super::{Callbacks, FailedAt, ResponseBody};
use crate::classify::{ClassifiedResponse, ClassifyResponse};
use futures_core::ready;
use http::Response;
use http_body::Body;
use pin_project::pin_project;
use std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Response future for [`LifeCycleHooks`].
///
/// [`LifeCycleHooks`]: crate::life_cycle_hooks::LifeCycleHooks
#[pin_project]
pub struct ResponseFuture<F, C, Callbacks, CallbacksData> {
    #[pin]
    pub(super) inner: F,
    pub(super) classifier: Option<C>,
    pub(super) callbacks: Option<Callbacks>,
    pub(super) callbacks_data: Option<CallbacksData>,
}

impl<F, C, ResBody, E, CallbacksT, CallbacksData> Future
    for ResponseFuture<F, C, CallbacksT, CallbacksData>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body,
    C: ClassifyResponse,
    CallbacksT: Callbacks<C::FailureClass, Data = CallbacksData>,
    E: fmt::Display + 'static,
{
    type Output =
        Result<Response<ResponseBody<ResBody, C::ClassifyEos, CallbacksT, CallbacksT::Data>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));

        let classifier = this
            .classifier
            .take()
            .expect("polled ResponseFuture after completion");

        let mut callbacks = this
            .callbacks
            .take()
            .expect("polled ResponseFuture after completion");

        let mut callbacks_data = this
            .callbacks_data
            .take()
            .expect("polled ResponseFuture after completion");

        match result {
            Ok(res) => {
                let classification = classifier.classify_response(&res);

                match classification {
                    ClassifiedResponse::Ready(classification) => {
                        callbacks.on_response(
                            &res,
                            ClassifiedResponse::Ready(classification),
                            &mut callbacks_data,
                        );

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            parts: None,
                        });

                        Poll::Ready(Ok(res))
                    }
                    ClassifiedResponse::RequiresEos(classify_eos) => {
                        callbacks.on_response(
                            &res,
                            ClassifiedResponse::RequiresEos(()),
                            &mut callbacks_data,
                        );

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            parts: Some((classify_eos, callbacks, callbacks_data)),
                        });

                        Poll::Ready(Ok(res))
                    }
                }
            }
            Err(err) => {
                let classification = classifier.classify_error(&err);
                callbacks.on_failure(FailedAt::Response, classification, callbacks_data);

                Poll::Ready(Err(err))
            }
        }
    }
}
