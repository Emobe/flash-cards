use std::cell::RefCell;

use crate::error::{ApiError, ErrorKind};
use crate::notice::{CoreEvent, EventSink, Notice, NullSink, Progress};

static NULL_SINK: NullSink = NullSink;

/// Supplied by the host to every method call: cancellation, progress, events and attachments.
pub struct OpContext<'a> {
    cancelled: &'a dyn Fn() -> bool,
    sink: &'a dyn EventSink,
    op: Option<u32>,
    attachment: Option<&'a [u8]>,
    reply_attachment: RefCell<Option<Vec<u8>>>,
}

impl<'a> OpContext<'a> {
    pub fn new(cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            cancelled,
            sink: &NULL_SINK,
            op: None,
            attachment: None,
            reply_attachment: RefCell::new(None),
        }
    }

    /// Never cancelled, no notices. For tests and hosts without cancellation.
    pub fn uncancellable() -> OpContext<'static> {
        OpContext::new(&|| false)
    }

    /// Where notices go. `op` is the client's operation ID, which tags progress notices.
    #[must_use]
    pub fn with_sink(mut self, sink: &'a dyn EventSink, op: Option<u32>) -> Self {
        self.sink = sink;
        self.op = op;
        self
    }

    /// The attachment the caller sent with the request.
    #[must_use]
    pub fn with_attachment(mut self, bytes: &'a [u8]) -> Self {
        self.attachment = Some(bytes);
        self
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

    /// Reports progress. Ignored when the caller gave no operation ID.
    pub fn progress(&self, progress: Progress) {
        if let Some(op) = self.op {
            self.sink.send(Notice::Progress { op, progress });
        }
    }

    pub fn emit(&self, event: CoreEvent) {
        self.sink.send(Notice::Event { event });
    }

    /// The attachment sent with the request, if any.
    pub fn attachment(&self) -> Option<&[u8]> {
        self.attachment
    }

    /// Sets the attachment returned with the reply.
    pub fn set_reply_attachment(&self, bytes: Vec<u8>) {
        *self.reply_attachment.borrow_mut() = Some(bytes);
    }

    pub(crate) fn take_reply_attachment(&self) -> Option<Vec<u8>> {
        self.reply_attachment.borrow_mut().take()
    }
}
