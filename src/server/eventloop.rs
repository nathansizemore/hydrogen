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
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError
};

use super::super::epoll;
use super::super::epoll::util::*;
use super::super::epoll::EpollEvent;
use super::super::simple_stream::nbetstream::NbetStream;

use server::stats;
use server::types::*;
use server::socket::Socket;


#[allow(dead_code)]
pub struct EventLoop {
    /// Handle to event loop process
    prox: JoinHandle<()>,
    /// Sender<T> to proc
    prox_tx: Sender<TcpStream>
}


impl EventLoop {

    /// Returns a new EventLoop
    pub fn new(server_tx: Sender<EventTuple>) -> EventLoop {
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
    fn start(rx: Receiver<TcpStream>, uspace_tx: Sender<EventTuple>) {
        // Master socket list
        let sockets = Arc::new(Mutex::new(LinkedList::<Socket>::new()));

        // Our error handler for this thread and it's children
        let (err_tx, err_rx): (Sender<()>, Receiver<()>) = channel();

        // Create an epoll instance
        let result = epoll::create1(0);
        if result.is_err() {
            let err = result.unwrap_err();
            error!("Unable to create epoll instance: {}", err);
            panic!()
        }
        let epoll_instance = result.unwrap();

        // Start the epoll thread
        let c_sockets = sockets.clone();
        let c_epoll_instance = epoll_instance.clone();
        thread::Builder::new()
            .name("EpollLoop".to_string())
            .spawn(move || {
                EventLoop::epoll_loop(uspace_tx, c_sockets, c_epoll_instance, err_rx);
            }).unwrap();

        for new_stream in rx.iter() {
            match NbetStream::new(new_stream) {
                Ok(s_stream) => {
                    trace!("New stream received in epoll.new_stream.rx");

                    // Add to master list
                    let socket = Socket::new(s_stream);

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

                    trace!("socket added to master list");

                    // Track the new connection
                    stats::conn_recv();

                    // Add to epoll
                    let mut event = EpollEvent {
                        data: s_fd as u64,
                        events: (event_type::EPOLLIN | event_type::EPOLLET | event_type::EPOLLRDHUP)
                    };
                    match epoll::ctl(epoll_instance, ctl_op::ADD, s_fd, &mut event) {
                        Ok(()) => trace!("New socket added to epoll list"),
                        Err(e) => warn!("Epoll CtrlError during add: {}", e)
                    };
                }
                Err(e) => warn!("Unable to create NbetStream: {}", e)
            };
        }

        // If we get here, shit is real bad. It means we have lost our
        // channel to the outside world.
        // Kill off event loop, so unwinding can begin
        error!("EpollLoop::start() thread ended unexpectedly");
        let _ = err_tx.send(());
    }

    /// Calls epoll_wait forever waiting for events to be fired up from kernel
    fn epoll_loop(uspace_tx: Sender<EventTuple>,
                  sockets: SocketList,
                  epoll_instance: RawFd,
                  err_rx: Receiver<()>) {

        // This is the maximum number of events we want to be notified of at one time
        let mut events = Vec::<EpollEvent>::with_capacity(100);
        unsafe { events.set_len(100); }

        loop {
            // Wait for epoll events
            trace!("epoll_wait...");
            match epoll::wait(epoll_instance, &mut events[..], -1) {
                Ok(num_events) => {
                    trace!("{} epoll event(s) received", num_events);
                    for x in 0..num_events {
                        let tx_clone = uspace_tx.clone();
                        let s_clone = sockets.clone();
                        let epfd_clone = epoll_instance.clone();

                        EventLoop::handle_epoll_event(epfd_clone,
                            &events[x as usize], s_clone, tx_clone);
                    }
                }
                Err(e) => error!("Error on epoll::wait(): {}", e)
            }

            // If we receive anything on this end or lose communication, we need to halt,
            // because we have nowhere to send up events, our "user space" channel is
            // gone
            match err_rx.try_recv() {
                Ok(_) => {
                    error!("Terminating epoll loop");
                    break;
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {}
                        _ => {
                            error!("Channel closed, terminating epoll loop");
                            break;
                        }
                    }
                }
            };
        }
        warn!("epoll_loop - thread finished");
    }

    /// Processes a read on the socket that is ready for reading
    ///
    /// An epoll event is passed to this function, with the fd that is ready to be read
    /// A traversal through the list of sockets is then performed in order to locate the
    /// NbetStream instance that is wrapping that file descriptor.
    /// After a read is performed on that socket, it's data will be passed back to the
    /// "user-space" section of the Server to process the payload however it needs to.
    ///
    /// TODO - Create a faster way to gain a safe reference to the NbetStream wrapping
    /// the fd instead of an O(n) lookup for every event. Currently lookup time is
    /// O(n) * MAX_EPOLL_EVENTS, where MAX_EPOLL_EVENTS is user defined in this function's
    /// caller.
    fn handle_epoll_event(epoll_instance: RawFd,
                          event: &EpollEvent,
                          sockets: SocketList,
                          uspace_tx: Sender<EventTuple>) {

        trace!("handle_epoll_event");

        // List of socket id's that produced an Err on read attempt
        // Cannot do it within the match block because the list will need
        // access to the mutex while this scope has the mutex, which will result
        // in a spinlock that will spin forever. And, that's a trip to bummer town
        let mut errd_socket_ids = Vec::<u32>::new();

        // The fd we are traversing the list for
        let fd = event.data;

        // We need a handle before the list gets locked and used
        let s_list_clone = sockets.clone();

        // TODO - Replace with recoverable version once into_inner() is stable
        // Unfortunately, this is the only stable way to use mutexes at the moment
        // Hopefully recovering from poisoning will be in 1.2
        // let mut s_guard = match sockets.lock() {
        //     Ok(guard) => guard,
        //     Err(poisoned) => poisoned.into_inner()
        // };
        let mut s_guard = sockets.lock().unwrap();
        let mut s_list = s_guard.deref_mut();
        for socket in s_list.iter_mut() {
            if socket.raw_fd() as u64 == fd {
                // We've found the socket for the event passed from epoll
                // Lets attempt to read from it
                match socket.read() {
                    Ok(()) => {
                        trace!("socket.read.Ok()");
                        for msg in socket.buffer().iter() {
                            let msg_for_debug = msg.clone();
                            match String::from_utf8(msg_for_debug) {
                                Ok(msg_str) => debug!("msg_as_string: {}", msg_str),
                                Err(e) => {
                                    // TODO - Determine if all things must be valid UTF-8
                                    // Potentially, bitbanging could be used which would
                                    // produce invalid UTF-8.
                                    //
                                    // For now, it just prints a warning
                                    warn!("Error converting to UTF-8: {}", e)
                                }
                            };

                            let list_handle = s_list_clone.clone();
                            let socket_clone = socket.clone();
                            let msg_clone = msg.clone();
                            let _ = uspace_tx.send((list_handle, socket_clone, msg_clone));

                            // Track message received event and num bytes received
                            stats::msg_recv();
                            stats::bytes_recv(msg.len());
                        }
                    }
                    Err(e) => {
                        debug!("Error reading socket: {}", e);
                        // We really don't care what happened, it all results in the same
                        // outcome - remove the socket from system...
                        errd_socket_ids.push(socket.id());
                        let s_clone = socket.clone();
                        let epfd_clone = epoll_instance.clone();
                        EventLoop::remove_socket_from_epoll(s_clone, epfd_clone);
                    }
                }
            }
        }

        // Remove any errd sockets from the master list of sockets
        EventLoop::remove_sockets_from_list(errd_socket_ids, &mut s_list);
    }

    /// Removes socket from the epoll watch list
    fn remove_socket_from_epoll(socket: Socket, epoll_instance: RawFd) {
        debug!("remove_socket_from_epoll");

        // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
        // a non-null pointer in event, even though this argument is ignored.
        // Since Linux 2.6.9, event can be specified as NULL when using
        // EPOLL_CTL_DEL.  Applications that need to be portable to kernels
        // before 2.6.9 should specify a non-null pointer in event.
        let mut event = EpollEvent {
            data: 0 as u64,
            events: 0 as u32
        };
        let s_fd = socket.raw_fd();

        // Depending on how fd are duplicated with .clone(), this may fail
        // If the failure case is CtlError::ENOENT, we do not care, because
        // epoll will clean the up the descriptor after they are all dropped from
        // program memory
        match epoll::ctl(epoll_instance, ctl_op::DEL, s_fd, &mut event) {
            Ok(()) => trace!("Socket removed from epoll watch list"),
            Err(e) => {
                match e {
                    CtlError::ENOENT => {
                        trace!("Fd not found in epoll, will remove when fd is syscall closed");
                    }
                    _ => warn!("Epoll CtrlError during del: {}", e)
                }
            }
        };
    }

    /// Removes socket_ids from master list
    fn remove_sockets_from_list(socket_ids: Vec<u32>, sockets: &mut LinkedList<Socket>) {
        debug!("remove_sockets_from_list");

        for socket_id in socket_ids.iter() {
            let mut socket_found = false;
            let mut index: usize = 1;
            for socket in sockets.iter() {
                if socket.id() == *socket_id {
                    socket_found = true;
                    break;
                }
                index += 1;
            }

            if !socket_found {
                debug!("Socket not found in list...?");
                continue;
            }

            if index == 1 {
                sockets.pop_front();
            } else {
                let mut split = sockets.split_off(index - 1);
                split.pop_front();
                sockets.append(&mut split);
            }

            // Track the disconnect
            stats::conn_lost();

            debug!("Socket removed sucessfully");
        }
    }
}
