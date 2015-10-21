// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::thread::JoinHandle;
use std::sync::Arc;
use std::ops::DerefMut;
use std::sync::mpsc::{channel, Sender, Receiver};

use types::*;
use socket::Socket;


pub struct WorkerThread {
    /// Sender to this threads receiver
    tx: Sender<EventHandlerFn>
}


impl WorkerThread {

    /// Creates a new worker thread
    pub fn new() -> WorkerThread {
        let (tx, rx): (Sender<EventHandlerFn>, Receiver<EventHandlerFn>) = channel();
        thread::Builder::new()
            .name("WorkerThread".to_string())
            .spawn(move || {
                WorkerThread::start(rx);
            }).unwrap();

        WorkerThread { tx: tx }
    }

    /// Returns a clone of this thread's Sender<T>
    pub fn sender(&self) -> Sender<EventHandlerFn> { self.tx.clone() }

    /// Starts the worker thread
    fn start(rx: Receiver<EventHandlerFn>) {
        for t in rx.iter() {
            
        }
    }
}
