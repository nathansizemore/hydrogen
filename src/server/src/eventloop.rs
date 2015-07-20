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
use std::ops::{DerefMut, Deref};
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

        // Our error handler for this thread and it's children
        let (err_tx, err_rx): (Sender<()>, Receiver<()>) = channel();

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
            .spawn(move || {
                EventLoop::epoll_loop(uspace_tx, c_sockets, c_epoll_instance, err_rx);
            }).unwrap();

        for new_stream in rx.iter() {
            match SimpleStream::new(new_stream) {
                Ok(s_stream) => {
                    // Add to master list
                    let mut socket = Socket::new(s_stream);

                    // TODO - Replace with recoverable version once into_inner() is stable
                    // Unfortunately, this is the only stable way to use mutexes at the moment
                    // Hopefully recovering from poisoning will be in 1.2
                    // let mut s_guard = match sockets.lock() {
                    //     Ok(guard) => guard,
                    //     Err(poisoned) => poisoned.into_inner()
                    // };
                    let mut s_guard = sockets.lock().unwrap();
                    let s_list = s_guard.deref_mut();
                    let s_fd = socket.raw_fd();
                    s_list.push_back(socket);

                    // Add to epoll
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
        // channel to the outside world.
        // Kill off event loop, so unwinding can begin
        err_tx.send(());
    }

    ///
    fn epoll_loop(uspace_tx: Sender<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>,
                  sockets: Arc<Mutex<LinkedList<Socket>>>,
                  epoll_instance: RawFd,
                  err_rx: Receiver<()>) {

        // This is the maximum number of events we want to be notified of at one time
        let mut events = Vec::<EpollEvent>::with_capacity(100);

        loop {
            // Wait for epoll events
            match epoll::wait(epoll_instance, &mut events[..], -1) {
                Ok(num_events) => {
                    for x in 0..num_events {
                        let tx_clone = uspace_tx.clone();
                        let s_clone = sockets.clone();
                        EventLoop::handle_epoll_event(tx_clone, &events[x as usize], s_clone);
                    }
                }
                Err(e) => {
                    panic!("Error on epoll::wait(): {}", e)
                }
            }

            // If we receive anything on this end or lose communication, we need to halt,
            // because we have no where to send up events, our "user space" channel is
            // gone
            match err_rx.try_recv() {
                Ok(_) => {
                    println!("Error, terminating epoll_loop");
                    break;
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {}
                        _ => {
                            println!("Error, terminating epoll_loo");
                            break;
                        }
                    }
                }
            }
        }
    }

    ///
    fn handle_epoll_event(uspace_tx: Sender<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>,
                          event: &EpollEvent,
                          sockets: Arc<Mutex<LinkedList<Socket>>>) {

        let fd = event.data;
        // TODO - Replace with recoverable version once into_inner() is stable
        // Unfortunately, this is the only stable way to use mutexes at the moment
        // Hopefully recovering from poisoning will be in 1.2
        // let mut s_guard = match sockets.lock() {
        //     Ok(guard) => guard,
        //     Err(poisoned) => poisoned.into_inner()
        // };
        let mut s_guard = sockets.lock().unwrap();
        let s_list = s_guard.deref_mut();

        for socket in s_list.iter_mut() {
            if socket.raw_fd() as u64 == fd {
                // We've found the socket for the event passed from epoll
                // Lets attempt to read from it
                match socket.read() {
                    Ok(()) => {
                        for msg in socket.buffer().iter() {

                        }
                    }
                    Err(e) => {
                        // TODO - Figure out how to handle
                    }
                }
            }
        }

    }
}
