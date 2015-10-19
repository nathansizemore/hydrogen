// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::thread::JoinHandle;
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender, Receiver};

use types::*;
use socket::Socket;


pub struct WorkerThread {
    /// Sender to this threads receiver
    tx: Sender<(EventFunction, SocketList, Socket, Vec<u8>)>
}


impl WorkerThread {

    /// Creates a new worker thread
    pub fn new() -> WorkerThread {
        let (tx, rx): (
            Sender<(EventFunction, SocketList, Socket, Vec<u8>)>,
            Receiver<(EventFunction, SocketList, Socket, Vec<u8>)>)
            = channel();

        thread::Builder::new()
            .name("WorkerThread".to_string())
            .spawn(move || {
                WorkerThread::start(rx);
            }).unwrap();

        WorkerThread {
            tx: tx
        }
    }

    /// Returns a clone of this thread's Sender<T>
    pub fn sender(&self) -> Sender<(EventFunction, SocketList, Socket, Vec<u8>)> {
        self.tx.clone()
    }

    /// Starts the worker thread
    fn start(rx: Receiver<(EventFunction, SocketList, Socket, Vec<u8>)>) {
        for (task, sockets, socket, buffer) in rx.iter() {
            (*task)(sockets, socket, buffer);
        }
    }
}
