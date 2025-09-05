// Copyright (C) 2025 SUSE LLC
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::Error;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(test)]
mod tests;

/// A message type passed over a [`std::sync::mpsc::channel`] for coordinating invoked threads.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum JobMessage {
    /// A job slot became available.
    SlotAvailable,

    /// One of the workers completed the last work item, or encountered an error.
    Completed,
}

/// A controller for running multiple jobs, similar to a make jobserver.
///
/// The controller facility consists of two components: a parent `JobControl` and its subordinate
/// [`JobSlots`] instances. Together, they provide functionality to run multiple jobs in parallel,
/// while at no time exceeding the maximum number of specified jobs.
///
/// The division into two components allows for two levels of control over the operation. It is
/// possible to run several tasks in parallel, each in its own thread, with each task utilizing
/// multiple threads internally.
///
/// The following example shows a typical use:
///
/// ```rust
/// use std::thread;
/// use suse_kabi_tools::burst;
/// use suse_kabi_tools::burst::JobControl;
///
/// let job_control_rc = JobControl::new(8);
/// let mut job_slots = JobControl::new_slots(&job_control_rc, 1);
/// let mut job_slots2 = JobControl::new_slots(&job_control_rc, 1);
///
/// let works = vec![0, 1, 2, 3, 4, 5, 6, 7];
/// let works2 = vec![8, 9, 10, 11, 12, 13, 14, 15];
///
/// thread::scope(|scope| {
///     scope.spawn(|| {
///         burst::run_jobs(
///             |work_idx| {
///                 println!("{}", works[work_idx]);
///                 Ok(())
///             },
///             works.len(),
///             &mut job_slots,
///         )
///     });
///
///     scope.spawn(|| {
///         burst::run_jobs(
///             |work_idx| {
///                 println!("{}", works2[work_idx]);
///                 Ok(())
///             },
///             works2.len(),
///             &mut job_slots2,
///         )
///     });
/// });
/// ```
///
/// If the operation has only one "top" task, then [`JobControl::new_simple()`] provides
/// a simplified interface for this scenario.
pub struct JobControl {
    /// The maximum number of jobs that can be run in parallel.
    maximum: i32,

    /// The current number of handed out job slots.
    active: i32,

    /// The number of [`JobSlots`] allocated by `JobControl` since its inception. This counter
    /// provides a trivial identifier allocator for any new [`JobSlots`]. Note that in typical
    /// usage, this counter is 1 or 2.
    num_children: usize,

    /// The sending ends of [`std::sync::mpsc::channel`], connecting `JobControl` with all its
    /// subordinate [`JobSlots`]. Each entry consists of a [`JobSlots`] identifier and its
    /// associated sender.
    listeners: HashMap<usize, mpsc::Sender<JobMessage>>,
}

impl JobControl {
    /// Creates a new job controller to manage the specified number of parallel jobs.
    pub fn new(maximum: i32) -> Arc<Mutex<Self>> {
        let job_control = JobControl {
            maximum,
            active: 0,
            num_children: 0,
            listeners: HashMap::new(),
        };
        Arc::new(Mutex::new(job_control))
    }

    /// Creates a new subordinate job controller with the specified number of reserved jobs.
    pub fn new_slots(this_rc: &Arc<Mutex<Self>>, reserved: i32) -> JobSlots {
        let mut job_control = this_rc.lock().unwrap();

        // SAFETY: The caller must not exceed the maximum number of jobs.
        assert!(reserved <= job_control.maximum - job_control.active);
        job_control.active += reserved;

        // SAFETY: The caller is expected to create only a limited number of subordinate job
        // controllers.
        assert!(job_control.num_children < usize::MAX);
        let child_id = job_control.num_children;
        job_control.num_children += 1;

        let (sender, receiver) = mpsc::channel();
        let maybe_listener = job_control.listeners.insert(child_id, sender.clone());
        // SAFETY: Each child identifier is uniquely allocated, ensuring no duplicate exists.
        assert!(maybe_listener.is_none());

        JobSlots {
            child_id,
            reserved,
            active: 0,
            parent: this_rc.clone(),
            sender,
            receiver,
        }
    }

    /// Creates a new parent job controller and its subordinate, with the latter being assigned all
    /// allowed jobs.
    ///
    /// This function is useful when only a single [`JobSlots`] instance is required.
    pub fn new_simple(maximum: i32) -> JobSlots {
        let job_control_rc = Self::new(maximum);
        Self::new_slots(&job_control_rc, maximum)
    }

    /// Unregisters a subordinate job controller and returns any of its reserved jobs.
    ///
    /// This function is invoked by [`JobSlots`] when it is dropped.
    fn unregister_slots(&mut self, child_id: usize, reserved: i32) {
        assert!(self.active >= reserved);
        self.active -= reserved;

        let maybe_listener = self.listeners.remove(&child_id);
        // SAFETY: A child listener is added when `new_slots()` creates a new `JobSlots` instance.
        assert!(maybe_listener.is_some());

        if reserved > 0 {
            self.broadcast_to_listeners(JobMessage::SlotAvailable);
        }
    }

    /// Attempts to acquire one job slot.
    ///
    /// Returns `Some(())` if a job slot was available, or `None` if all slots are currently in use.
    fn acquire_one(&mut self) -> Option<()> {
        if self.active < self.maximum {
            self.active += 1;
            Some(())
        } else {
            None
        }
    }

    /// Releases one job slot.
    fn release_one(&mut self) {
        assert!(self.active > 0);
        self.active -= 1;
        self.broadcast_to_listeners(JobMessage::SlotAvailable);
    }

    /// Sends a message to all subordinate job controllers.
    fn broadcast_to_listeners(&mut self, message: JobMessage) {
        for listener in self.listeners.values() {
            // SAFETY: The code tracks only active listeners, ensuring that `send()` cannot return
            // a `SendError`.
            listener.send(message).unwrap();
        }
    }
}

impl Drop for JobControl {
    fn drop(&mut self) {
        // SAFETY: The caller must explicitly release all acquired job slots.
        assert!(self.active == 0);

        // SAFETY: Each subordinate job controller holds a reference to its parent, ensuring that
        // the parent can only be dropped after all subordinate job controllers have been destroyed.
        // Additionally, each subordinate job controller must properly unregister itself by calling
        // `unregister_slots()`.
        assert!(self.listeners.is_empty());
    }
}

/// A subordinate job controller.
///
/// Refer the description of [`JobControl`] for more details.
pub struct JobSlots {
    /// The identifier of the instance to its [`JobControl`] parent.
    child_id: usize,

    /// The number of reserved slots.
    reserved: i32,

    /// The number of slots currently in use.
    active: i32,

    /// A reference to the parent [`JobControl`].
    parent: Arc<Mutex<JobControl>>,

    /// The sending end of our [`std::sync::mpsc::channel`]. The sender is cloned and provided to
    /// individual workers to allow them to send the [`JobMessage::Completed`] message.
    sender: mpsc::Sender<JobMessage>,

    /// The receiving end of our [`std::sync::mpsc::channel`].
    receiver: mpsc::Receiver<JobMessage>,
}

impl JobSlots {
    /// Attempts to acquire one job slot.
    ///
    /// Returns `Some(())` if a job slot was available, or `None` if all slots are currently in use.
    pub fn acquire_one(&mut self) -> Option<()> {
        let res = if self.active < self.reserved {
            Some(())
        } else {
            let mut job_control = self.parent.lock().unwrap();
            job_control.acquire_one()
        };
        if res.is_some() {
            self.active += 1;
        }
        res
    }

    /// Releases one job slot.
    pub fn release_one(&mut self) {
        assert!(self.active > 0);

        if self.active > self.reserved {
            let mut job_control = self.parent.lock().unwrap();
            job_control.release_one();
        }
        self.active -= 1;
    }

    /// Ensures that at least one reserved slot is available and blocks until a slot can be
    /// acquired.
    ///
    /// This function should be used by logic that utilizes `JobSlots` when running with a single
    /// thread. It guarantees that such control code is permitted to perform any real work without
    /// exceeding the maximum number of jobs.
    pub fn ensure_one_reserved(&mut self) {
        // SAFETY: This function should be only used when running with a single implicit thread.
        assert!(self.active == 0);

        if self.reserved > 0 {
            return;
        }

        loop {
            {
                let mut job_control = self.parent.lock().unwrap();
                if job_control.acquire_one().is_some() {
                    self.reserved = 1;
                    break;
                }
            }

            match self.recv_ctrl_msg() {
                JobMessage::SlotAvailable => continue,
                JobMessage::Completed => {
                    panic!("Received JobMessage::Completed when running with a single thread")
                }
            }
        }
    }

    /// Receives a message from the control channel. Blocks if no message is available.
    pub fn recv_ctrl_msg(&mut self) -> JobMessage {
        // SAFETY: At a minimum, the `self` instance and its `JobControl` parent are connected to
        // the channel, ensuring that `recv()` cannot return a `RecvError`.
        self.receiver.recv().unwrap()
    }

    /// Retrieves a sender for the control channel.
    pub fn get_ctrl_sender(&mut self) -> mpsc::Sender<JobMessage> {
        self.sender.clone()
    }
}

impl Drop for JobSlots {
    fn drop(&mut self) {
        // SAFETY: The caller must explicitly release all acquired job slots.
        assert!(self.active == 0);

        // Unregister this instance from the parent controller and return any reserved jobs.
        let mut job_control = self.parent.lock().unwrap();
        job_control.unregister_slots(self.child_id, self.reserved)
    }
}

/// Invokes the specified function for each work in parallel.
///
/// The process function is invoked for each value in the range `0..num_works`. The operation is
/// executed in parallel, using the provided [`JobSlots`] for coordination.
pub fn run_jobs<F: Fn(usize) -> Result<(), Error> + Send + Sync>(
    process_fun: F,
    num_works: usize,
    job_slots: &mut JobSlots,
) -> Result<(), Error> {
    // Note that when running the worker threads, the main control thread is not considered as
    // contributing to the maximum number of jobs.

    let next_work_idx = AtomicUsize::new(0);

    thread::scope(|scope| {
        let mut workers = Vec::new();

        loop {
            // Check if the operation has been completed.
            if next_work_idx.load(Ordering::Relaxed) >= num_works {
                break;
            }

            // Attempt to spawn a new worker to process more work.
            if job_slots.acquire_one().is_some() {
                let worker_sender = job_slots.get_ctrl_sender();
                workers.push(scope.spawn(|| {
                    // Run the worker, fetching new work one by one until everything is completed.
                    let worker_sender = worker_sender;

                    loop {
                        let work_idx = next_work_idx.fetch_add(1, Ordering::Relaxed);
                        if work_idx >= num_works {
                            // SAFETY: The `job_slots` receiver has a longer lifetime than the
                            // thread, ensuring that `send()` cannot return a `SendError`.
                            worker_sender.send(JobMessage::Completed).unwrap();
                            return Ok(());
                        }

                        if let Err(err) = process_fun(work_idx) {
                            // An error occurred. Short-circuit all the remaining work.
                            next_work_idx.store(num_works, Ordering::Relaxed);
                            worker_sender.send(JobMessage::Completed).unwrap();
                            return Err(err);
                        }
                    }
                }));

                continue;
            }

            // Wait for a new job slot to become available, or for the work to be completed.
            job_slots.recv_ctrl_msg();
        }

        // Join all the worker threads. Return the first error if any is found, others are silently
        // swallowed which is ok.
        let mut maybe_err = None;
        for worker in workers {
            let res = worker.join().unwrap();
            if res.is_err() && maybe_err.is_none() {
                maybe_err = res.err();
            }
            job_slots.release_one();
        }
        if let Some(err) = maybe_err {
            return Err(err);
        }

        Ok(())
    })
}
