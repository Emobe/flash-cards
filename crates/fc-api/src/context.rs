use crate::error::{ApiError, ErrorKind};

/// Supplied by the host to every method call. Step 0.3b adds progress and attachments.
pub struct OpContext<'a> {
    cancelled: &'a dyn Fn() -> bool,
}

impl<'a> OpContext<'a> {
    pub fn new(cancelled: &'a dyn Fn() -> bool) -> Self {
        Self { cancelled }
    }

    /// Never cancelled. For tests and hosts without cancellation.
    pub fn uncancellable() -> OpContext<'static> {
        OpContext {
            cancelled: &|| false,
        }
    }

    /// Long methods call this regularly. Returns `cancelled` once cancellation was requested.
    pub fn checkpoint(&self) -> Result<(), ApiError> {
        if (self.cancelled)() {
            Err(ApiError::new(
                ErrorKind::Cancelled,
                "The operation was cancelled.",
            ))
        } else {
            Ok(())
        }
    }
}
