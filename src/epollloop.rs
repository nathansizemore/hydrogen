// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::net::{TcpListener, ToSocketAddrs};
use std::ops::DerefMut;
use std::os::unix::io::RawFd;
use std::collections::LinkedList;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver
};

use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use simple_stream::nbetstream::NbetStream;
use stats;
use types::*;
use socket::Socket;
use resources::ResourcePool;


// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLIN      - Available for read
// EPOLLRDHUP   - Connection has been closed
// EPOLLONESHOT - After an event has been received, and reported, do not track
//                further changes until explicitly told to do so.
const EVENTS: u32 = (
    event_type::EPOLLET | // Set fd to EdgeTrigger mode
    event_type::EPOLLIN |
    event_type::EPOLLRDHUP |
    event_type::EPOLLONESHOT);

pub fn begin<T, K>(address: T, handler: Box<K>) where
    T: ToSocketAddrs + Send + 'static,
    K: EventHandler + Send + Sync + 'static {
    // Master socket list
    let sockets = Arc::new(Mutex::new(LinkedList::<Socket>::new()));

    // Epoll instance
    let result = epoll::create1(0);
    if result.is_err() {
        let err = result.unwrap_err();
        error!("Unable to create epoll instance: {}", err);
        panic!()
    }
    let epfd = result.unwrap();

    // EpollEvent handler thread
    let epfd1 = epfd.clone();
    let sockets1 = sockets.clone();
    let (tx_ep_event, rx_ep_event): (Sender<EpollEvent>, Receiver<EpollEvent>) = channel();
    let safe_handler = Arc::new(Mutex::new(*handler));
    thread::Builder::new()
        .name("EpollEvent Handler".to_string())
        .spawn(move || {
            epoll_event_handler(epfd1, rx_ep_event, sockets1, safe_handler);
        }).unwrap();

    // Epoll wait thread
    let epfd2 = epfd.clone();
    thread::Builder::new()
        .name("Epoll Wait".to_string())
        .spawn(move || {
            epoll_wait_loop(epfd2, tx_ep_event);
        }).unwrap();

    // New connection thread
    let epfd3 = epfd.clone();
    let sockets2 = sockets.clone();
    let prox = thread::Builder::new()
        .name("TCP Incoming Listener".to_string())
        .spawn(move || {
            listen(address, epfd3, sockets2);
        }).unwrap();

    // Stay alive forever
    let _ = prox.join();
}

fn listen<T: ToSocketAddrs>(address: T, epfd: RawFd, sockets: SocketList) {
    let listener = TcpListener::bind(address).unwrap();
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                trace!("New connection received");
                // Create new socket
                match NbetStream::new(stream) {
                    Ok(nb_stream) => {
                        let socket = Socket::new(nb_stream);
                        let socket_ptr;
                        // Add to master list
                        { // Begin Mutex lock
                            let mut guard = match sockets.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => {
                                    warn!("SocketList Mutex failed, using anyway...");
                                    poisoned.into_inner()
                                }
                            };
                            let socket_list = guard.deref_mut();
                            socket_list.push_back(socket.clone());
                            socket_ptr = socket_list.back_mut().unwrap() as *mut Socket;
                        } // End Mutex lock

                        // Add to epoll
                        let sfd = socket.raw_fd();
                        let mut event = EpollEvent {
                            data: socket_ptr as u64,
                            events: EVENTS
                        };
                        match epoll::ctl(epfd, ctl_op::ADD, sfd, &mut event) {
                            Ok(()) => trace!("New socket added to epoll list"),
                            Err(e) => warn!("Epoll CtrlError during add: {}", e)
                        };
                    }
                    Err(e) => warn!("Error creating nbstream: {}", e)
                }
            }
            Err(e) => warn!("Error encountered during TCP connection: {}", e)
        }
    }
    drop(listener);
}

fn epoll_wait_loop(epfd: RawFd, tx_handler: Sender<EpollEvent>) {
    // Maximum number of events we want to be notified of at one time
    let mut events = Vec::<EpollEvent>::with_capacity(100);
    unsafe { events.set_len(100); }

    loop {
        match epoll::wait(epfd, &mut events[..], -1) {
            Ok(num_events) => {
                for x in 0..num_events as usize {
                    let _ = tx_handler.send(events[x]);
                }
            }
            Err(e) => {
                error!("Error on epoll::wait(): {}", e);
                panic!()
            }
        }
    }
}

fn epoll_event_handler(epfd: RawFd,
                       rx: Receiver<EpollEvent>,
                       sockets: SocketList,
                       handler: SafeHandler) {
    // Types of events we care about
    const READ_EVENT: u32 = event_type::EPOLLIN;
    const DROP_EVENT: u32 = event_type::EPOLLRDHUP | event_type::EPOLLHUP;

    // Thread pool
    let mut pool = ResourcePool::new();

    for event in rx.iter() {
        // Get a pointer to the socket in the list
        let socket;
        { // Begin Mutex lock
            let _ = match sockets.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("SocketList Mutex failed, using anyway...");
                    poisoned.into_inner()
                }
            };
            socket = event.data as *mut Socket;
        } // End Mutex lock

        if (event.events & DROP_EVENT) > 0 {
            let t_epfd = epfd.clone();
            let socket_list = sockets.clone();
            let t_handler = handler.clone();
            let socketfd = unsafe { (*socket).raw_fd() };
            let socketid = unsafe { (*socket).id() };

            pool.run(move || {
                epoll_remove_fd(t_epfd, socketfd);
                remove_socket_from_list(socketid, socket_list.clone());
                { // EventHandler Mutex locked
                    let mut guard = match t_handler.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => {
                            warn!("EventHandler Mutex failed, using anyway...");
                            poisoned.into_inner()
                        }
                    };
                    let e_handler = guard.deref_mut();
                    e_handler.on_socket_closed(socketid);
                } // EventHandler Mutex unlocked
            });
        } else if (event.events & READ_EVENT) > 0 {
            let t_epfd = epfd.clone();
            let mut s_clone = unsafe { (*socket).clone() };
            let socketfd = s_clone.raw_fd();
            let socket_list = sockets.clone();
            let t_handler = handler.clone();
            let socket_ptr = socket as u64;

            pool.run(move || {
                match s_clone.read() {
                    Ok(_) => {
                        for msg in s_clone.buffer().iter() {
                            let debug_msg = msg.clone();
                            match String::from_utf8(debug_msg) {
                                Ok(msg_str) => trace!("msg: {}", msg_str),
                                Err(_) => trace!("msg not UTF-8")
                            };

                            { // EventHandler Mutex locked
                                let mut guard = match t_handler.lock() {
                                    Ok(guard) => guard,
                                    Err(poisoned) => {
                                        warn!("EventHandler Mutex failed, using anyway...");
                                        poisoned.into_inner()
                                    }
                                };
                                let e_handler = guard.deref_mut();
                                e_handler.on_data_received(s_clone.clone(),
                                    socket_list.clone(), msg.clone());
                            } // EventHandler Mutex unlocked

                            // Track message received event and num bytes received
                            stats::msg_recv();
                            stats::bytes_recv(msg.len());

                            // Add socket's fd back to epoll watch list
                            let mut event = EpollEvent {
                                data: socket_ptr,
                                events: EVENTS
                            };
                            match epoll::ctl(t_epfd.clone(), ctl_op::MOD, socketfd, &mut event) {
                                Ok(_) => trace!("Socket back in epoll list"),
                                Err(e) => warn!("Epoll CtrlError during mod: {}", e)
                            };
                        }
                    }
                    Err(e) => {
                        debug!("Error reading socket: {}", e);
                        // We really don't care what happened, it all results in the same
                        // outcome - remove the socket from system...
                        let s_id = s_clone.id();
                        epoll_remove_fd(t_epfd, socketfd);
                        remove_socket_from_list(s_id, socket_list.clone());
                        let mut guard = match t_handler.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => {
                                warn!("SocketList Mutex failed, using anyway...");
                                poisoned.into_inner()
                            }
                        };
                        let e_handler = guard.deref_mut();
                        e_handler.on_socket_closed(s_id);
                    }
                };
            });
        } else { warn!("Unknown epoll event received: {}", event.data); continue; }
    }
}

fn epoll_remove_fd(epfd: RawFd, fd: RawFd) {
    debug!("remove_socket_from_epoll");

    // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
    // a non-null pointer in event, even though this argument is ignored.
    // Since Linux 2.6.9, event can be specified as NULL when using
    // EPOLL_CTL_DEL.  Applications that need to be portable to kernels
    // before 2.6.9 should specify a non-null pointer in event.
    let mut rm_event = EpollEvent {
        data: 0 as u64,
        events: 0 as u32
    };

    // Depending on how fds are duplicated with .clone(), this may fail.
    //
    // If the failure case is CtlError::ENOENT, we do not care, because
    // epoll will clean the up the descriptor after they are all dropped from
    // program memory
    match epoll::ctl(epfd, ctl_op::DEL, fd, &mut rm_event) {
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
fn remove_socket_from_list(socket_id: u32, sockets: SocketList) {
    debug!("remove_socket_from_list");

    let mut s_guard = match sockets.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("SocketList Mutex was poisoned, using anyway");
            poisoned.into_inner()
        }
    };
    let s_list = s_guard.deref_mut();

    let mut s_found = false;
    let mut index = 1usize;
    for s in s_list.iter() {
        if s.id() == socket_id {
            s_found = true;
            break;
        }
        index += 1;
    }

    if !s_found { return; }

    if index == 1 {
        s_list.pop_front();
    } else {
        let mut split = s_list.split_off(index - 1);
        split.pop_front();
        s_list.append(&mut split);
    }

    stats::conn_lost();

    trace!("socket removed from list");
}
