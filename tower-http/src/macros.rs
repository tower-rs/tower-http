#[allow(unused_macros)]
macro_rules! define_inner_service_accessors {
    () => {
        /// Gets a reference to the underlying service.
        pub fn get_ref(&self) -> &S {
            &self.inner
        }

        /// Gets a mutable reference to the underlying service.
        pub fn get_mut(&mut self) -> &mut S {
            &mut self.inner
        }

        /// Consumes `self`, returning the underlying service.
        pub fn into_inner(self) -> S {
            self.inner
        }
    };
}

#[allow(unused_macros)]
macro_rules! opaque_body {
    ($(#[$m:meta])* pub type $name:ident = $actual:ty;) => {
        opaque_body! {
            $(#[$m])* pub type $name<> = $actual;
        }
    };

    ($(#[$m:meta])* pub type $name:ident<$($param:ident),*> = $actual:ty;) => {
        pin_project_lite::pin_project! {
            $(#[$m])*
            pub struct $name<$($param),*> {
                #[pin]
                pub(crate) inner: $actual
            }
        }

        impl<$($param),*> $name<$($param),*> {
            pub(crate) fn new(inner: $actual) -> Self {
                Self { inner }
            }
        }

        impl<$($param),*> http_body::Body for $name<$($param),*> {
            type Data = <$actual as http_body::Body>::Data;
            type Error = <$actual as http_body::Body>::Error;

            #[inline]
            fn poll_frame(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
                self.project().inner.poll_frame(cx)
            }

            #[inline]
            fn is_end_stream(&self) -> bool {
                http_body::Body::is_end_stream(&self.inner)
            }

            #[inline]
            fn size_hint(&self) -> http_body::SizeHint {
                http_body::Body::size_hint(&self.inner)
            }
        }
    };
}

#[allow(unused_macros)]
macro_rules! opaque_future {
    ($(#[$m:meta])* pub type $name:ident<$($param:ident),+> = $actual:ty;) => {
        pin_project_lite::pin_project! {
            $(#[$m])*
            pub struct $name<$($param),+> {
                #[pin]
                inner: $actual
            }
        }

        impl<$($param),+> $name<$($param),+> {
            pub(crate) fn new(inner: $actual) -> Self {
                Self {
                    inner
                }
            }
        }

        impl<$($param),+> std::fmt::Debug for $name<$($param),+> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_tuple(stringify!($name)).field(&format_args!("...")).finish()
            }
        }

        impl<$($param),+> std::future::Future for $name<$($param),+>
        where
            $actual: std::future::Future,
        {
            type Output = <$actual as std::future::Future>::Output;
            #[inline]
            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
                self.project().inner.poll(cx)
            }
        }
    }
}

/// Evaluate `$call` at most once every `$interval` per call site.
///
/// Uses a monotonic clock and atomic timestamp to rate-limit without locks.
/// Adapted from dial9-tokio-telemetry's rate_limit module.
// TODO: Once MSRV >= 1.70, switch to OnceLock<Instant> for monotonic timing.
// See: https://github.com/dial9-rs/dial9/blob/6772039/dial9-tokio-telemetry/src/rate_limit.rs
#[allow(unused_macros)]
macro_rules! rate_limited {
    ($interval:expr, $call:expr) => {{
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        static NEXT_CALL: AtomicU64 = AtomicU64::new(0);

        let interval: Duration = $interval;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        let next = NEXT_CALL.load(Ordering::Relaxed);
        if now >= next {
            let new_next = now.saturating_add(interval.as_secs());
            if NEXT_CALL
                .compare_exchange(next, new_next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                $call;
            }
        }
    }};
}
