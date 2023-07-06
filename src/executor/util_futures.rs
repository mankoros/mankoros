use crate::{arch::within_sum, tools::errors::Async};
use alloc::vec::Vec;
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

pub struct SumFuture<F: Future> {
    future: F,
}

impl<F: Future> Future for SumFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        within_sum(|| unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().future).poll(cx) })
    }
}

pub fn within_sum_async<F: Future>(future: F) -> SumFuture<F> {
    SumFuture { future }
}

pub struct AnyFuture<'a, T> {
    futures: Vec<Async<'a, T>>,
    has_returned: bool,
}

impl<'a, T> AnyFuture<'a, T> {
    pub fn new() -> Self {
        Self {
            futures: Vec::new(),
            has_returned: false,
        }
    }
    pub fn push(&mut self, future: Async<'a, T>) {
        self.futures.push(future);
    }

    pub fn new_with(futures: Vec<Async<'a, T>>) -> Self {
        Self {
            futures,
            has_returned: false,
        }
    }
}

impl<T> Future for AnyFuture<'_, T> {
    type Output = (usize, T);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        if this.has_returned {
            return Poll::Pending;
        }

        for (i, future) in this.futures.iter_mut().enumerate() {
            let result = unsafe { Pin::new_unchecked(future).poll(cx) };
            if let Poll::Ready(ret) = result {
                this.has_returned = true;
                return Poll::Ready((i, ret));
            }
        }

        Poll::Pending
    }
}
