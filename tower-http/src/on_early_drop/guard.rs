//! Implementation of the OnEarlyDropGuard.

/// A struct that executes a closure when dropped.
///
/// The closure will only be executed if `completed()` has not been called before dropping.
///
/// # Examples
///
/// ```
/// use tower_http::on_early_drop::guard::OnEarlyDropGuard;
/// use std::cell::RefCell;
/// use std::rc::Rc;
///
/// // Create a counter we can check if the callback was called
/// let counter = Rc::new(RefCell::new(0));
/// let counter_clone = counter.clone();
///
/// {
///     // Create a value that will be consumed by the FnOnce closure
///     let data = String::from("This string will be consumed");
///
///     // Create a guard with a closure that consumes `data` (FnOnce behavior)
///     let _guard = OnEarlyDropGuard::new(move || {
///         // Here we're consuming `data` by printing its length - can only be done once
///         println!("Length of consumed data: {}", data.len());
///         // Still update the counter for test verification
///         *counter_clone.borrow_mut() += 1;
///     });
///     // Guard goes out of scope here and should call the callback
/// }
///
/// assert_eq!(*counter.borrow(), 1);
/// ```
///
/// When completed is called, the callback won't run:
///
/// ```
/// use tower_http::on_early_drop::guard::OnEarlyDropGuard;
/// use std::cell::RefCell;
/// use std::rc::Rc;
///
/// // Create a counter we can check if the callback was called
/// let counter = Rc::new(RefCell::new(0));
/// let counter_clone = counter.clone();
///
/// {
///     // Create a value that would be consumed by the FnOnce closure
///     let data = String::from("This string would be consumed");
///
///     // Create a guard that would consume `data` when dropped
///     let mut guard = OnEarlyDropGuard::new(move || {
///         // This closure would consume `data` but won't be called
///         println!("This won't be printed: {}", data);
///         *counter_clone.borrow_mut() += 1;
///     });
///
///     // Mark as completed, so callback shouldn't run
///     guard.completed();
///     // Guard goes out of scope here but won't execute the FnOnce closure
/// }
///
/// assert_eq!(*counter.borrow(), 0);
/// ```
#[derive(Debug)]
pub struct OnEarlyDropGuard<F>
where
    F: FnOnce(),
{
    callback: Option<F>,
    completed: bool,
}

impl<F> OnEarlyDropGuard<F>
where
    F: FnOnce(),
{
    /// Creates a new `OnEarlyDropGuard` instance with the provided callback.
    ///
    /// The callback will be executed when the struct is dropped, unless `completed()` has been called.
    pub fn new(callback: F) -> Self {
        Self {
            callback: Some(callback),
            completed: false,
        }
    }

    /// Marks the operation as completed, preventing the callback from being called when dropped.
    ///
    /// Call this method when the operation has been completed successfully and you don't want
    /// the early drop callback to be executed.
    pub fn completed(&mut self) {
        self.completed = true;
    }
}

impl<F> Drop for OnEarlyDropGuard<F>
where
    F: FnOnce(),
{
    fn drop(&mut self) {
        // Only call the callback if not marked as completed
        if !self.completed {
            // Take the callback to ensure it can only be called once
            let callback = self.callback.take().expect("callback is only used on drop");
            callback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_callback_called_on_drop() {
        // Create a counter we can check if the callback was called
        let counter = Rc::new(RefCell::new(0));
        let counter_clone = counter.clone();

        {
            // Create a guard with a closure that increments the counter
            let _guard = OnEarlyDropGuard::new(move || {
                *counter_clone.borrow_mut() += 1;
            });
            // Guard goes out of scope here and should call the callback
        }

        assert_eq!(*counter.borrow(), 1);
    }

    #[test]
    fn test_callback_not_called_when_completed() {
        // Create a counter we can check if the callback was called
        let counter = Rc::new(RefCell::new(0));
        let counter_clone = counter.clone();

        {
            // Create a guard with a counter-incrementing closure
            let mut guard = OnEarlyDropGuard::new(move || {
                *counter_clone.borrow_mut() += 1;
            });

            // Mark as completed, so callback shouldn't run
            guard.completed();
            // Guard goes out of scope here, but the callback won't be executed
        }

        assert_eq!(*counter.borrow(), 0);
    }
}
