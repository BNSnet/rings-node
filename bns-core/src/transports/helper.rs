use anyhow::anyhow;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

#[derive(Default)]
pub struct State {
    pub completed: bool,
    pub successed: Option<bool>,
    pub waker: Option<std::task::Waker>,
}

#[derive(Default)]
pub struct Promise(pub Arc<Mutex<State>>);

impl Promise {
    pub fn state(&self) -> Arc<Mutex<State>> {
        Arc::clone(&self.0)
    }
}

impl Future for Promise {
    type Output = Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.0.lock().unwrap();
        if state.completed {
            return match &state.successed {
                Some(true) => Poll::Ready(Ok(())),
                _ => Poll::Ready(Err(anyhow!("failed on promise"))),
            };
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
