//! Futures that hold a thread-local scope while they are polled.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Run `future` inside the scope `enter` opens around each of its polls; the
/// guard `enter` returns closes the scope when it drops, even by a panic.
///
/// The scope is installed per poll, never held across an await, so it follows
/// the future when a work-stealing runtime moves it to another thread. `enter`,
/// and whatever it holds, is dropped inside the poll that completes the
/// future: dropped later, outside any runtime, a pooled connection it holds
/// would panic.
pub(crate) fn per_poll<F, S, G>(future: F, enter: S) -> impl Future<Output = F::Output>
where
    F: Future,
    S: FnMut() -> G + Unpin,
{
    PerPoll {
        future: Box::pin(future),
        enter: Some(enter),
    }
}

struct PerPoll<F, S> {
    future: Pin<Box<F>>,
    enter: Option<S>,
}

impl<F, S, G> Future for PerPoll<F, S>
where
    F: Future,
    S: FnMut() -> G + Unpin,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let result = match this.enter.as_mut() {
            Some(enter) => {
                let _scope = enter();
                this.future.as_mut().poll(cx)
            }
            None => this.future.as_mut().poll(cx),
        };
        if result.is_ready() {
            this.enter = None;
        }
        result
    }
}
