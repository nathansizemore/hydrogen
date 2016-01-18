// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use types::*;
use num_cpus;
use workerthread::WorkerThread;


/// Provides access to and manages worker threads
pub struct ResourcePool {
    /// Collection of threads
    w_threads: Vec<WorkerThread>,
    /// Backlog queue
    next_worker: usize,
}


impl ResourcePool {
    /// Creates a new pool of worker threads
    pub fn new(num_workers: u8) -> ResourcePool {
        let mut num_workers = num_workers;
        if num_workers < 1 {
            warn!("Must at least have one worker thread. Creating 1");
            num_workers = 1;
        }

        // Initialize woker threads
        let mut w_threads = Vec::<WorkerThread>::with_capacity(num_workers as usize);
        for _ in 0..num_workers {
            w_threads.push(WorkerThread::new());
        }

        ResourcePool {
            w_threads: w_threads,
            next_worker: 0,
        }
    }

    /// Runs the passed function
    pub fn run<T: Fn() + Send + Sync + 'static>(&mut self, task: T) {
        if self.next_worker == self.w_threads.len() {
            self.next_worker = 0;
        }

        let event = Event(Box::into_raw(Box::new(task)));
        let _ = self.w_threads[self.next_worker].sender().send(event);
        self.next_worker += 1;
    }
}
