// Copyright (C) 2025 SUSE LLC <petr.pavlu@suse.com>
// SPDX-License-Identifier: GPL-2.0-or-later

use super::*;
use crate::{assert_ok, assert_parse_err};
use std::thread;
use std::time::Duration;

#[test]
fn job_control_acquire_release() {
    // Check that the number of active slots is correctly tracked by the acquire and release
    // operations on `JobControl`.
    let job_control_rc = JobControl::new(3);
    let mut job_control = job_control_rc.lock().unwrap();
    assert_eq!(job_control.active, 0);

    assert_eq!(job_control.acquire_one(), Some(()));
    assert_eq!(job_control.active, 1);
    assert_eq!(job_control.acquire_one(), Some(()));
    assert_eq!(job_control.active, 2);
    assert_eq!(job_control.acquire_one(), Some(()));
    assert_eq!(job_control.active, 3);

    assert_eq!(job_control.acquire_one(), None);
    assert_eq!(job_control.active, 3);

    job_control.release_one();
    assert_eq!(job_control.active, 2);
    job_control.release_one();
    assert_eq!(job_control.active, 1);
    job_control.release_one();
    assert_eq!(job_control.active, 0);
}

#[test]
fn job_slots_acquire_release() {
    // Check that the number of active slots is correctly tracked by the acquire and release
    // operations on `JobSlots`.
    let job_control_rc = JobControl::new(3);
    assert_eq!(job_control_rc.lock().unwrap().active, 0);

    let mut job_slots = JobControl::new_slots(&job_control_rc, 1);
    let mut job_slots2 = JobControl::new_slots(&job_control_rc, 0);
    assert_eq!(job_control_rc.lock().unwrap().active, 1);
    assert_eq!(job_slots.active, 0);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 0);
    assert_eq!(job_slots2.reserved, 0);

    assert_eq!(job_slots.acquire_one(), Some(()));
    assert_eq!(job_control_rc.lock().unwrap().active, 1);
    assert_eq!(job_slots.active, 1);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 0);
    assert_eq!(job_slots2.reserved, 0);

    assert_eq!(job_slots2.acquire_one(), Some(()));
    assert_eq!(job_control_rc.lock().unwrap().active, 2);
    assert_eq!(job_slots.active, 1);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 1);
    assert_eq!(job_slots2.reserved, 0);

    assert_eq!(job_slots.acquire_one(), Some(()));
    assert_eq!(job_control_rc.lock().unwrap().active, 3);
    assert_eq!(job_slots.active, 2);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 1);
    assert_eq!(job_slots2.reserved, 0);

    assert_eq!(job_slots.acquire_one(), None);
    assert_eq!(job_slots2.acquire_one(), None);
    assert_eq!(job_control_rc.lock().unwrap().active, 3);
    assert_eq!(job_slots.active, 2);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 1);
    assert_eq!(job_slots2.reserved, 0);

    job_slots2.release_one();
    assert_eq!(job_control_rc.lock().unwrap().active, 2);
    assert_eq!(job_slots.active, 2);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 0);
    assert_eq!(job_slots2.reserved, 0);

    job_slots.release_one();
    assert_eq!(job_control_rc.lock().unwrap().active, 1);
    assert_eq!(job_slots.active, 1);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 0);
    assert_eq!(job_slots2.reserved, 0);

    job_slots.release_one();
    assert_eq!(job_control_rc.lock().unwrap().active, 1);
    assert_eq!(job_slots.active, 0);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 0);
    assert_eq!(job_slots2.reserved, 0);
}

#[test]
fn job_slots_ensure_one_reserved() {
    // Check that when a task runs with a single thread, it can wait for a reserved slot to avoid
    // exceeding the maximum number of jobs.
    let job_control_rc = JobControl::new(1);
    assert_eq!(job_control_rc.lock().unwrap().active, 0);

    let job_slots = JobControl::new_slots(&job_control_rc, 1);
    let job_slots2 = JobControl::new_slots(&job_control_rc, 0);
    assert_eq!(job_control_rc.lock().unwrap().active, 1);
    assert_eq!(job_slots.active, 0);
    assert_eq!(job_slots.reserved, 1);
    assert_eq!(job_slots2.active, 0);
    assert_eq!(job_slots2.reserved, 0);

    let vec_mutex = Mutex::new(Vec::new());

    thread::scope(|scope| {
        scope.spawn(|| {
            let mut job_slots = job_slots;

            // The thread should proceed because `job_slots` has a reserved slot since its
            // inception.
            job_slots.ensure_one_reserved();

            thread::sleep(Duration::from_millis(100));
            vec_mutex.lock().unwrap().push(1);
        });

        scope.spawn(|| {
            let mut job_slots2 = job_slots2;

            // The thread should block because `job_slots2` needs to wait on `job_slots` to release
            // its reserved slot. It is the only slot available.
            job_slots2.ensure_one_reserved();

            vec_mutex.lock().unwrap().push(2);
        });
    });

    assert_eq!(job_control_rc.lock().unwrap().active, 0);

    // Check that the threads operated in the correct order.
    let vec = vec_mutex.into_inner().unwrap();
    assert_eq!(vec, vec![1, 2]);
}

#[test]
fn run_jobs_one_task() {
    // Check the basic functionality of `burst::run_jobs()` when only a single task is present.
    let mut job_slots = JobControl::new_simple(8);
    let vec_mutex = Mutex::new(Vec::new());

    let result = run_jobs(
        |work_idx| {
            vec_mutex.lock().unwrap().push(work_idx);
            Ok(())
        },
        100,
        &mut job_slots,
    );
    assert_ok!(result);

    let mut vec = vec_mutex.into_inner().unwrap();
    assert_eq!(vec.len(), 100);
    vec.sort();
    for i in 0..vec.len() {
        assert_eq!(vec[i], i);
    }
}

#[test]
fn run_jobs_two_tasks() {
    // Check the basic functionality of `burst::run_jobs()` when two tasks compete for slots.
    let job_control_rc = JobControl::new(8);
    let job_slots = JobControl::new_slots(&job_control_rc, 1);
    let job_slots2 = JobControl::new_slots(&job_control_rc, 1);
    let vec_mutex = Mutex::new(Vec::new());

    let result = thread::scope(|scope| -> Result<(), Error> {
        let thread = scope.spawn(|| {
            let mut job_slots = job_slots;

            run_jobs(
                |work_idx| {
                    vec_mutex.lock().unwrap().push(work_idx);
                    Ok(())
                },
                100,
                &mut job_slots,
            )
        });

        let thread2 = scope.spawn(|| {
            let mut job_slots2 = job_slots2;

            run_jobs(
                |work_idx| {
                    vec_mutex.lock().unwrap().push(100 + work_idx);
                    Ok(())
                },
                100,
                &mut job_slots2,
            )
        });

        thread.join().unwrap()?;
        thread2.join().unwrap()?;
        Ok(())
    });
    assert_ok!(result);

    let mut vec = vec_mutex.into_inner().unwrap();
    assert_eq!(vec.len(), 200);
    vec.sort();
    for i in 0..vec.len() {
        assert_eq!(vec[i], i);
    }
}

#[test]
fn run_jobs_error() {
    // Check that `burst::run_jobs()` terminates the entire operation early if any job encounters an
    // error.
    let mut job_slots = JobControl::new_simple(8);
    let vec_mutex = Mutex::new(Vec::new());

    let result = run_jobs(
        |work_idx| {
            if work_idx == 10 {
                return Err(Error::new_parse("#10 is bad"));
            };
            thread::sleep(Duration::from_millis(100));
            vec_mutex.lock().unwrap().push(work_idx);
            Ok(())
        },
        100,
        &mut job_slots,
    );
    assert_parse_err!(result, "#10 is bad");

    let vec = vec_mutex.into_inner().unwrap();

    // When work index #10 is reached, other threads processing smaller work indices should still
    // add them to the output vector. Some more may also be added depending on the timing.
    assert!(vec.len() >= 10);

    // The operation is expected to short-circuit all running threads when the first error is
    // encountered. Given the manual delay introduced by `thread::sleep()`, no thread should ever
    // reach the end of the entire operation.
    assert!(vec.len() < 100);
}
