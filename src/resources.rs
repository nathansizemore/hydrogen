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
    pub fn new() -> ResourcePool {
        // At this time, the following threads have been accounted for:
        // 1.) System resources/applications
        // 2.) Incoming connections
        // 3.) Epoll wait event loop
        // 4.) Epoll event handler
        //
        // So, in theory, we should be allocating enough workers for num cpus - 4
        // But, this might be in a dev environment where keeping one thread per core
        // is not such a huge rule. We'll just assume the user knows that they will only
        // get 1 worker thread for anything less than 5 cores reported by the system.
        let cpus = num_cpus::get();
        let mut num_workers: i8 = (cpus as i8) - 4;
        if num_workers < 1 {
            warn!("Yo - asumming a dev env. Only 1 worker thread will be used");
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
