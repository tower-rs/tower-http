//! Service implementation for the OnEarlyDrop middleware.
//!
//! This module provides the [`OnEarlyDropService`] which wraps another service and monitors
//! for early client disconnections during request processing.
//!
//! The service uses the [`OnEarlyDropGuard`]
//! to track request lifecycle and executes a callback when a request is dropped before completion.

use crate::on_early_drop::future::OnEarlyDropFuture;
use crate::on_early_drop::guard::OnEarlyDropGuard;
use std::marker::PhantomData;
use std::task::{Context, Poll};
use tower_service::Service;

/// A middleware [`Service`] responsible for handling early drop scenarios.
///
/// This service wraps an inner service and monitors requests for early client disconnections.
/// When a client disconnects before receiving the complete response, a provided callback
/// function will be executed, allowing for logging, metrics collection, or other cleanup tasks.
///
/// This service is typically applied using the [`OnEarlyDropLayer`](crate::on_early_drop::layer::OnEarlyDropLayer).
///
/// # Type Parameters
///
/// * `CallbackFactory` - The callback factory type, a function that produces callbacks from requests
/// * `Callback` - The callback type, a function that will be executed if a request is dropped early
/// * `S` - The inner service type being wrapped
/// * `Request` - The request type being processed by the service
/// * `Response` - The response type produced by the service
#[derive(Debug)]
pub struct OnEarlyDropService<CallbackFactory, Callback, S, Request, Response>
where
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
{
    inner: S,
    callback_factory: CallbackFactory,
    _marker: PhantomData<(Callback, Request, Response)>,
}

/// Manual implementation of Clone for OnEarlyDropService.
///
/// This implementation avoids the additional bounds that would be automatically
/// added by #[derive(Clone)]. The derive macro would add unnecessary `T: Clone` bounds
/// on all generic parameters, including `Request` and `Response`, even though these
/// types are never actually cloned in the implementation.
///
/// By manually implementing Clone, we only require `S` and `CallbackFactory` to be
/// cloneable, which are the only fields we actually clone. This allows the service
/// to be cloned even when the `Request` and `Response` types don't implement Clone.
///
/// See: <https://doc.rust-lang.org/std/clone/trait.Clone.html#derivable> for more details
/// on why manual implementations are sometimes needed to avoid unnecessary bounds.
impl<CallbackFactory, Callback, S, Request, Response> Clone
    for OnEarlyDropService<CallbackFactory, Callback, S, Request, Response>
where
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            callback_factory: self.callback_factory.clone(),
            _marker: PhantomData,
        }
    }
}

impl<CallbackFactory, Callback, S, Request, Response>
    OnEarlyDropService<CallbackFactory, Callback, S, Request, Response>
where
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
{
    /// Creates a new `OnEarlyDropService` with the given inner service and callback factory.
    ///
    /// # Parameters
    ///
    /// * `inner` - The inner service to wrap
    /// * `callback_factory` - A factory function that creates callback closures for each request
    pub fn new(inner: S, callback_factory: CallbackFactory) -> Self {
        Self {
            inner,
            callback_factory,
            _marker: PhantomData,
        }
    }
}

impl<CallbackFactory, Callback, S, Request, Response> Service<Request>
    for OnEarlyDropService<CallbackFactory, Callback, S, Request, Response>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = OnEarlyDropFuture<S::Future, Callback>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        // Create the guard that will call the callback if dropped
        let guard = OnEarlyDropGuard::new((self.callback_factory)(&req));

        // Call the inner service
        let future = self.inner.call(req);

        // Wrap the future and guard in our OnEarlyDropFuture
        OnEarlyDropFuture::new(future, guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future;
    use http::{Request, Response, StatusCode};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::{sleep, timeout};
    use tower::{service_fn, ServiceExt};

    #[tokio::test]
    async fn test_service_calls_inner_service() {
        // Create a simple handler that returns a 200 OK
        let inner_service = service_fn(|_req: Request<()>| {
            future::ready(Ok::<_, std::io::Error>(
                Response::builder().status(StatusCode::OK).body(()).unwrap(),
            ))
        });

        // Create a simple callback factory
        let callback_factory = |_req: &Request<()>| || {};

        // Create the OnEarlyDropService
        let service = OnEarlyDropService::<_, _, _, Request<()>, Response<()>>::new(
            inner_service,
            callback_factory,
        );

        // Create a test request
        let req = Request::builder()
            .uri("http://example.com/")
            .body(())
            .unwrap();

        // Call the service using oneshot and get the response
        let response = service.oneshot(req).await.unwrap();

        // Verify that the inner service was called and returned the expected response
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_callback_not_called_when_completed() {
        // Track callback invocations
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        // Create a simple handler
        let inner_service = service_fn(|_req: Request<()>| {
            future::ready(Ok::<_, std::io::Error>(Response::new(())))
        });

        // Create a callback factory that sets the flag when called
        let callback_factory = move |_req: &Request<()>| {
            let called = called_clone.clone();
            move || {
                let mut called = called.lock().unwrap();
                *called = true;
            }
        };

        // Create the OnEarlyDropService
        let service = OnEarlyDropService::<_, _, _, Request<()>, Response<()>>::new(
            inner_service,
            callback_factory,
        );

        // Create a test request
        let req = Request::builder()
            .uri("http://example.com/")
            .body(())
            .unwrap();

        // Call the service using oneshot and get the response
        let _response = service.oneshot(req).await.unwrap();

        // The callback should not have been called because the request completed
        assert_eq!(*called.lock().unwrap(), false);
    }

    #[tokio::test]
    async fn test_service_clone() {
        // Create counters to track which service instance is called
        let original_counter = Arc::new(Mutex::new(0));
        let clone_counter = Arc::new(Mutex::new(0));

        // Create callback counters to track which callback is executed
        let original_callback_called = Arc::new(Mutex::new(false));
        let clone_callback_called = Arc::new(Mutex::new(false));

        // Clone the counters for use in the closures
        let orig_counter_clone = original_counter.clone();
        let clone_counter_clone = clone_counter.clone();
        let orig_cb_called_clone = original_callback_called.clone();
        // This clone is used indirectly through create_callback_factory for the cloned service
        let _clone_cb_called_clone = clone_callback_called.clone();

        // Create a parameterized service function that increments the appropriate counter
        let create_test_service = |counter: Arc<Mutex<i32>>| {
            service_fn(move |_req: Request<()>| {
                let counter = counter.clone();
                async move {
                    // Increment the counter when the service is called
                    let mut count = counter.lock().unwrap();
                    *count += 1;

                    Ok::<_, std::io::Error>(
                        Response::builder().status(StatusCode::OK).body(()).unwrap(),
                    )
                }
            })
        };

        // Create a callback factory that captures which instance was called
        let create_callback_factory = |callback_flag: Arc<Mutex<bool>>| {
            move |_req: &Request<()>| {
                let called = callback_flag.clone();
                move || {
                    let mut flag = called.lock().unwrap();
                    *flag = true;
                }
            }
        };

        // Create the original service with its counter
        let original_service = create_test_service(orig_counter_clone);
        let original_callback = create_callback_factory(orig_cb_called_clone);

        // Create the OnEarlyDropService
        let service = OnEarlyDropService::<_, _, _, Request<()>, Response<()>>::new(
            original_service,
            original_callback,
        );

        // Clone the service
        let _original_cloned = service.clone(); // Keep this to test Clone trait works

        // Create a new service with the clone counter to track separate metrics
        let cloned_service = OnEarlyDropService::<_, _, _, Request<()>, Response<()>>::new(
            create_test_service(clone_counter_clone),
            create_callback_factory(clone_callback_called.clone()),
        );

        // Create two test requests
        let req1 = Request::builder()
            .uri("http://example.com/original")
            .body(())
            .unwrap();

        let req2 = Request::builder()
            .uri("http://example.com/clone")
            .body(())
            .unwrap();

        // Call both services using oneshot
        let response1 = service.oneshot(req1).await.unwrap();
        let response2 = cloned_service.oneshot(req2).await.unwrap();

        // Verify both services returned OK responses
        assert_eq!(response1.status(), StatusCode::OK);
        assert_eq!(response2.status(), StatusCode::OK);

        // Verify each counter was incremented exactly once
        assert_eq!(*original_counter.lock().unwrap(), 1);
        assert_eq!(*clone_counter.lock().unwrap(), 1);

        // Verify neither callback was called (since both requests completed)
        assert_eq!(*original_callback_called.lock().unwrap(), false);
        assert_eq!(*clone_callback_called.lock().unwrap(), false);
    }

    #[tokio::test]
    async fn test_service_send_across_tasks() {
        // Create a simple test service that can be sent across tasks
        let inner_service = service_fn(move |_req: Request<()>| async move {
            Ok::<_, std::io::Error>(Response::builder().status(StatusCode::OK).body(()).unwrap())
        });

        // Create a callback factory that just prints a message when request is dropped early
        let callback_factory = move |req: &Request<()>| {
            let path = req.uri().path().to_string();
            move || {
                println!("Request not finished: {}", path);
            }
        };

        // Create the service
        let service = OnEarlyDropService::<_, _, _, Request<()>, Response<()>>::new(
            inner_service,
            callback_factory,
        );

        // Create a test request
        let req = Request::builder()
            .uri("http://example.com/test")
            .body(())
            .unwrap();

        // Create a channel to pass the response back from the task
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Spawn a new task and move the service into it
        let handle = tokio::spawn(async move {
            // This will fail to compile if the service doesn't implement Send
            let response = service.oneshot(req).await;

            // Send the result back through the channel
            let _ = tx.send(response);

            // Task completes naturally
        });

        // Wait for the response from the spawned task
        let response = rx
            .await
            .expect("Task should send a response")
            .expect("Service call should succeed");

        // Wait for the task to complete
        handle.await.expect("Task should complete successfully");

        // Verify the response is as expected
        assert_eq!(response.status(), StatusCode::OK);

        // The request should complete normally and no callback should be triggered
        // Task completion is verified by the test framework since we await the handle
    }

    #[tokio::test]
    async fn test_callback_called_when_dropped_early() {
        // Create a counter to track callback invocations
        let callback_counter = Arc::new(Mutex::new(0));
        let callback_counter_clone = callback_counter.clone();

        // Create an inner service that sleeps indefinitely, ensuring timeout
        let inner_service = service_fn(|_req: Request<()>| async {
            // Sleep for a very long time (simulating a hanging request)
            sleep(Duration::from_secs(60)).await;
            Ok::<_, std::io::Error>(Response::new(()))
        });

        // Create a callback factory that increments the counter when called
        let callback_factory = move |_req: &Request<()>| {
            let counter = callback_counter_clone.clone();
            move || {
                let mut count = counter.lock().unwrap();
                *count += 1;
                println!("Callback executed, count now: {}", *count);
            }
        };

        // Create the OnEarlyDropService
        let service = OnEarlyDropService::<_, _, _, Request<()>, Response<()>>::new(
            inner_service,
            callback_factory,
        );

        // Create a test request
        let req = Request::builder()
            .uri("http://example.com/test")
            .body(())
            .unwrap();

        // Call the service with a short timeout, ensuring the future will be dropped
        let result = timeout(Duration::from_millis(100), service.oneshot(req)).await;

        // Verify that the operation timed out
        assert!(result.is_err(), "Expected timeout error");

        // Add a small delay to ensure the callback has time to execute after drop
        sleep(Duration::from_millis(10)).await;

        // Verify the callback was called exactly once
        assert_eq!(
            *callback_counter.lock().unwrap(),
            1,
            "Callback should have been called once"
        );
    }
}
