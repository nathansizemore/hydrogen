// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.
//
// This Source Code Form is "Incompatible
// With Secondary Licenses", as defined by
// the Mozilla Public License, v. 2.0.


use std::sync::Arc;

use super::FpWrapper;
use super::types::*;
use super::socket::Socket;
use super::num_cpus;
use super::workerthread::WorkerThread;


/// Provides access to and manages worker threads
pub struct ResourcePool {
    /// Collection of threads
    w_threads: Vec<WorkerThread>,
    /// Backlog queue
    next_worker: usize
}


impl ResourcePool {

    /// Creates a new pool of worker threads
    pub fn new() -> ResourcePool {
        // This is designed to only be used in a multi-core/cpu environment.
        // Defaults to only one thread per cpu/core.
        //
        // One thread is already accounted for with the incoming connection
        // listener. One thread is accounted for with the event loop.
        // We need one cpu left available for anything else the system needs
        // to do that is not related to our program. That means, by default,
        // the number of worker threads is (totalCPUs - 3).
        //
        // Obviously, if this machine is running less than 4 cores, we should
        // panic and thow out an error message to indicate we need more POWER!!
        let num = num_cpus::get();
        if num < 4 {
            error!("Need a minimum of 4 CPUs to run");
            panic!()
        }

        // Initialize woker threads
        let mut w_threads = Vec::<WorkerThread>::with_capacity(num);
        for _ in 0..num {
            w_threads.push(WorkerThread::new());
        }

        ResourcePool {
            w_threads: w_threads,
            next_worker: 0
        }
    }

    /// Runs the passed function
    pub fn run(&mut self, fp_wrapper: Arc<FpWrapper>,
        sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
            
        if self.next_worker == self.w_threads.len() {
            self.next_worker = 0;
        }

        let _ = self.w_threads[self.next_worker].sender().send((
            fp_wrapper, sockets, socket, buffer
        ));
        self.next_worker += 1;
    }
}
