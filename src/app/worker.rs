//! A bounded, latest-request worker. Computation never holds the mailbox lock.
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

struct Mailbox<I, O> {
    pending: Option<(u64, I)>,
    completed: Option<(u64, O)>,
    closed: bool,
}

pub(super) struct Worker<I, O> {
    mailbox: Arc<(Mutex<Mailbox<I, O>>, Condvar)>,
    generation: u64,
}

impl<I: Send + 'static, O: Send + 'static> Worker<I, O> {
    pub(super) fn spawn(
        name: &str,
        mut compute: impl FnMut(I) -> O + Send + 'static,
    ) -> std::io::Result<Self> {
        let mailbox = Arc::new((
            Mutex::new(Mailbox {
                pending: None,
                completed: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let shared = Arc::clone(&mailbox);
        thread::Builder::new().name(name.into()).spawn(move || {
            loop {
                let (lock, wake) = &*shared;
                let mut state = wake
                    .wait_while(lock.lock().unwrap(), |state| {
                        state.pending.is_none() && !state.closed
                    })
                    .unwrap();
                if state.closed {
                    break;
                }
                let (generation, input) = state.pending.take().expect("pending work");
                drop(state);
                let output = compute(input);
                let mut state = lock.lock().unwrap();
                if state.closed {
                    break;
                }
                let previous = state.completed.replace((generation, output));
                drop(state);
                // Large discarded documents are freed on the worker thread.
                drop(previous);
            }
        })?;
        Ok(Self {
            mailbox,
            generation: 0,
        })
    }

    pub(super) fn request(&mut self, input: I) -> u64 {
        self.generation += 1;
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap().pending = Some((self.generation, input));
        wake.notify_one();
        self.generation
    }

    pub(super) fn take(&self) -> Option<(u64, O)> {
        self.mailbox.0.lock().unwrap().completed.take()
    }
}

impl<I, O> Drop for Worker<I, O> {
    fn drop(&mut self) {
        let (lock, wake) = &*self.mailbox;
        lock.lock().unwrap().closed = true;
        wake.notify_one();
        // Exiting the UI must not join an in-flight parser or Git command.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn slow_work_does_not_block_requests_and_intermediate_requests_are_skipped() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut worker = Worker::spawn("test-worker", move |input| {
            started_tx.send(input).unwrap();
            if input == 1 {
                release_rx.recv().unwrap();
            }
            input
        })
        .unwrap();
        worker.request(1);
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        worker.request(2);
        worker.request(3);
        release_tx.send(()).unwrap();
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 3);
        assert!(started_rx.try_recv().is_err());
    }
}
