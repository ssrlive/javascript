use crossbeam_channel::{Receiver, Sender, select, unbounded};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum TimerCommand {
    Schedule { id: usize, when: Instant },
    Cancel(usize),
}

/// Spawn the timer thread and return (cmd_sender, expired_receiver).
pub fn spawn_timer_thread() -> (Sender<TimerCommand>, Receiver<usize>) {
    let (cmd_tx, cmd_rx) = unbounded::<TimerCommand>();
    let (expired_tx, expired_rx) = unbounded::<usize>();

    thread::Builder::new()
        .name("js-timer-thread".to_string())
        .spawn(move || {
            // min-heap of (Instant, id)
            let mut heap: BinaryHeap<Reverse<(Instant, usize)>> = BinaryHeap::new();
            // canceled ids
            let mut canceled: HashSet<usize> = HashSet::new();

            loop {
                // determine next timeout
                let timeout = if let Some(Reverse((when, _id))) = heap.peek().cloned() {
                    let now = Instant::now();
                    if when <= now {
                        // immediate; don't wait
                        Some(Duration::from_millis(0))
                    } else {
                        Some(when - now)
                    }
                } else {
                    None
                };

                // wait for either a command or timeout
                if let Some(t) = timeout {
                    if t.is_zero() {
                        // pop all expired items
                        let now = Instant::now();
                        while let Some(Reverse((when, id))) = heap.peek().cloned() {
                            if when <= now {
                                heap.pop();
                                if !canceled.remove(&id) {
                                    // notify main thread of expiry
                                    if let Err(e) = expired_tx.send(id) {
                                        log::warn!("Failed to send expired timer id: {e:?}");
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        // loop back to recompute timeout
                        continue;
                    }

                    select! {
                        recv(cmd_rx) -> msg => match msg {
                            Ok(TimerCommand::Schedule { id, when }) => {
                                heap.push(Reverse((when, id)));
                            }
                            Ok(TimerCommand::Cancel(id)) => {
                                canceled.insert(id);
                            }
                            Err(_) => {
                                break; // channel closed
                            }
                        },
                        default(t) => {
                            // timed wait: busy-wait using sleep for small t
                            // but if t is large, we can block on recv with timeout by using recv_timeout
                            // we emulate by trying recv with timeout
                            match cmd_rx.recv_timeout(t) {
                                Ok(TimerCommand::Schedule { id, when }) => heap.push(Reverse((when, id))),
                                Ok(TimerCommand::Cancel(id)) => { canceled.insert(id); }
                                Err(_) => { /* timeout or disconnected */ }
                            }
                        }
                    }
                } else {
                    // no timers scheduled: block until a command arrives
                    match cmd_rx.recv() {
                        Ok(TimerCommand::Schedule { id, when }) => heap.push(Reverse((when, id))),
                        Ok(TimerCommand::Cancel(id)) => {
                            canceled.insert(id);
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
            }
        })
        .expect("failed to spawn timer thread");

    (cmd_tx, expired_rx)
}
