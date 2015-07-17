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
use std::os::unix::io::RawFd;
use std::collections::LinkedList;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError
};

use super::epoll;
use super::epoll::util::*;
use super::epoll::EpollEvent;
use super::num_cpus;
use super::socket::Socket;
use super::simple_stream::SimpleStream;


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
        // TODO - Figure out if this is needed to ensure collisions aren't made?
        let mut ids = Arc::new(Mutex::new(LinkedList::<u32>::new()));


        // Create an epoll instance
        let result = epoll::create1(0);
        if result.is_err() {
            let err = result.unwrap_err();
            panic!("Unable to create epoll instance: {}", err)
        }
        let epoll_instance = result.unwrap();

        // Start the epoll thread
        let c_sockets = sockets.clone();
        let c_epoll_instance = epoll_instance.clone();
        let epoll_prox = thread::Builder::new()
            .name("EpollLoop".to_string())
            .scoped(move || {
                EventLoop::epoll_loop(uspace_tx, c_sockets, c_epoll_instance);
            }).unwrap();

        for new_stream in rx.iter() {
            match SimpleStream::new(new_stream) {
                Ok(s_stream) => {
                    // Add to master list
                    let mut socket = Socket::new(s_stream);
                    let mut s_guard = match sockets.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner()
                    };
                    let s_list = s_guard.deref_mut();
                    s_list.push_back(socket);

                    // Add to epoll
                    let s_fd = socket.raw_fd();
                    let event = Box::new(EpollEvent {
                        data: s_fd as u64,
                        events: (event_type::EPOLLIN | event_type::EPOLLET)
                        });
                    match epoll::ctl(epoll_instance, ctl_op::ADD, s_fd, event) {
                        Ok(()) => {},
                        Err(e) => println!("Epoll CtrlError: {}", e)
                    };
                }
                Err(e) => {
                    // TODO - write error to log file
                    println!("Error: {}", e);
                }
            };
        }

        // If we get here, shit is real bad. It means we have lost our
        // channel to the outside world. Nothing else to do besides panic!
        // Because the epoll loop is a scoped thread belonging to this scope,
        // we can panic here and cause it to unwind also
        panic!("Error - ")
    }

    fn epoll_loop(uspace_tx: Sender<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>, sockets: Arc<Mutex<LinkedList<Socket>>>, epoll_instace: RawFd) {
        loop {

        }
    }
}
