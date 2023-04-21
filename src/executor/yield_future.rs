use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub struct YieldFuture(bool);

impl Future for YieldFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 {
            return Poll::Ready(());
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

pub fn yield_now() -> YieldFuture {
    YieldFuture(false)
}
