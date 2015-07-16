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


use std::thread;
use std::thread::JoinHandle;
use std::net::TcpStream;
use std::ops::DerefMut;
use std::collections::LinkedList;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError
};

use super::num_cpus;
use super::socket::Socket;


pub struct EventLoop {
    /// Handle to event loop process
    prox: JoinHandle<()>,
    /// Sender<T> to proc
    prox_tx: Sender<TcpStream>
}


impl EventLoop {

    /// Returns a new EventLoop
    pub fn new(server_tx: Sender<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>) -> EventLoop {
        let (tx, rx): (Sender<TcpStream>, Receiver<TcpStream>) = channel();
        let prox = thread::Builder::new()
            .name("EventLoop".to_string())
            .spawn(move || {
                EventLoop::start(rx, server_tx);
            }).unwrap();

        EventLoop {
            prox: prox,
            prox_tx: tx
        }
    }

    /// Returns a clone of this EventLoop's Sender<TcpStream> channel
    pub fn sender(&self) -> Sender<TcpStream> {
        self.prox_tx.clone()
    }

    /// Main event loop
    fn start(rx: Receiver<TcpStream>, uspace_tx: Sender<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>) {
        // Master socket list
        let mut sockets = Arc::new(Mutex::new(LinkedList::<Socket>::new()));

        // Master id list
        let mut ids = Arc::new(Mutex::new(LinkedList::<u32>::new()));






    }
}
