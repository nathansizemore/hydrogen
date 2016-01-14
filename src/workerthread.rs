// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::sync::mpsc::{Sender, Receiver, channel};

use types::*;


pub struct WorkerThread {
    /// Sender to this threads receiver
    tx: Sender<Event>,
}


impl WorkerThread {
    /// Creates a new worker thread
    pub fn new() -> WorkerThread {
        let (tx, rx): (Sender<Event>, Receiver<Event>) = channel();
        thread::Builder::new()
            .name("WorkerThread".to_string())
            .spawn(move || {
                WorkerThread::start(rx);
            })
            .unwrap();

        WorkerThread { tx: tx }
    }

    /// Returns a clone of this thread's Sender<T>
    pub fn sender(&self) -> Sender<Event> {
        self.tx.clone()
    }

    /// Starts the worker thread
    fn start(rx: Receiver<Event>) {
        for event in rx.iter() {
            let Event(ptr) = event;
            unsafe {
                (*ptr)();
                Box::new(Box::from_raw(ptr));
            }
        }
    }
}
