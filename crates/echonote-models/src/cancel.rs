use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// Stops a download; clones share the flag. What was downloaded stays for next time.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        let notified = self.0.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// `future`'s output, or `None` as soon as `cancel` fires.
pub(crate) async fn unless_cancelled<T>(
    cancel: &Cancel,
    future: impl Future<Output = T>,
) -> Option<T> {
    let cancelled = std::pin::pin!(cancel.cancelled());
    let future = std::pin::pin!(future);
    match futures_util::future::select(cancelled, future).await {
        futures_util::future::Either::Left(_) => None,
        futures_util::future::Either::Right((value, _)) => Some(value),
    }
}
