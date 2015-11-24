// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::{ptr, mem, thread};
use std::io::Error;
use std::ops::DerefMut;
use std::os::unix::io::RawFd;
use std::collections::LinkedList;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver
};

use libc;
use errno::errno;
use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use stream::socket::Socket;
use stream::nbstream::Nbstream;
use stats;
use types::*;
use resources::ResourcePool;
use libc::{c_ushort, c_ulong, c_uint};
use config::Config;


extern "C" {
    fn shim_htons(hostshort: c_ushort) -> c_ushort;
    fn shim_htonl(addr: c_uint) -> c_uint;
    fn shim_inaddr_any() ->  c_ulong;
}

// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLIN      - Available for read
// EPOLLONESHOT - After an event has been received, and reported, do not track
//                further changes until explicitly told to do so.
const EVENTS: u32 = (
    event_type::EPOLLET | // Set fd to EdgeTrigger mode
    event_type::EPOLLIN |
    event_type::EPOLLONESHOT;

pub fn begin<T>(config: Config, handler: Box<T>) where
    T: EventHandler + Send + Sync + 'static {
    // Master socket list
    let sockets = Arc::new(Mutex::new(LinkedList::<Nbstream>::new()));

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
    let streams1 = sockets.clone();
    let (tx_ep_event, rx_ep_event): (Sender<EpollEvent>, Receiver<EpollEvent>) = channel();
    let safe_handler = Arc::new(Mutex::new(*handler));
    thread::Builder::new()
        .name("EpollEvent Handler".to_string())
        .spawn(move || {
            epoll_event_handler(epfd1, rx_ep_event, streams1, safe_handler);
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
    let streams2 = sockets.clone();
    let prox = thread::Builder::new()
        .name("TCP Incoming Listener".to_string())
        .spawn(move || {
            listen(config, epfd3, streams2);
        }).unwrap();

    // Stay alive forever
    let _ = prox.join();
}

fn listen(config: Config, epfd: RawFd, streams: StreamList) {
    unsafe {
        let server_addr = libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: shim_htons(config.port),
            sin_addr: libc::in_addr {
                s_addr: shim_htonl(shim_inaddr_any() as u32)
            },
            sin_zero: [0u8; 8]
        };

        let server_fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        let serv_addr: *const libc::sockaddr = mem::transmute(&server_addr);
        let bind_result = libc::bind(server_fd, serv_addr,
            mem::size_of::<libc::sockaddr_in>() as u32);
        if bind_result < 0 {
            error!("Binding fd: {}", Error::from_raw_os_error(errno().0 as i32));
            return;
        }

        let listen_result = libc::listen(server_fd, 100);
        if listen_result < 0 {
            error!("Listening: {}", Error::from_raw_os_error(errno().0 as i32));
        }

        loop {
            let client_fd = libc::accept(server_fd,
                ptr::null_mut() as *mut libc::sockaddr, ptr::null_mut() as *mut u32);

            let socket = Socket {
                fd: client_fd
            };
            let stream = Nbstream::new(socket).unwrap();

            { // Begin Mutex lock
                let mut guard = match streams.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("StreamList Mutex failed, using anyway...");
                        poisoned.into_inner()
                    }
                };
                let stream_list = guard.deref_mut();
                stream_list.push_back(stream);
            } // End Mutex lock

            // Add to epoll
            let add_res = epoll::ctl(epfd, ctl_op::ADD, client_fd, &mut EpollEvent {
                data: client_fd as u64,
                events: EVENTS
            });
            if add_res.is_err() {
                error!("CtrlError during add: {}", add_res.unwrap_err());
            } else {
                trace!("New fd added to epoll");
                stats::conn_recv();
            }
        }
    }
}

fn epoll_wait_loop(epfd: RawFd, tx_handler: Sender<EpollEvent>) {
    // Maximum number of events we want to be notified of at one time
    let mut events = Vec::<EpollEvent>::with_capacity(100);
    unsafe { events.set_len(100); }

    loop {
        match epoll::wait(epfd, &mut events[..], 250) {
            Ok(num_events) => {
                trace!("{} events to process", num_events);
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
                       streams: StreamList,
                       handler: SafeHandler) {
    // Types of events we care about
    const READ_EVENT: u32 = event_type::EPOLLIN;
    const DROP_EVENT: u32 = event_type::EPOLLRDHUP | event_type::EPOLLHUP | event_type::EPOLLERR;

    // Thread pool
    let mut pool = ResourcePool::new();

    for event in rx.iter() {
        let event_fd = event.data as RawFd;
        trace!("received epoll event for fd: {}", event_fd);

        let stream;
        { // Begin Mutex lock
            // Find the stream the event was for
            let mut guard = match streams.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    warn!("StreamList Mutex was poisoned, using anyway");
                    poisoned.into_inner()
                }
            };
            let list = guard.deref_mut();

            let mut found = false;
            let mut index = 1usize;
            for s in list.iter() {
                let fd = s.as_raw_fd();
                trace!("event_fd: {} fd: {}", event_fd, fd);
                if s.as_raw_fd() == event_fd {
                    found = true;
                    break;
                }
                index += 1;
            }

            if !found {
                error!("Could not find fd in stream list?");
                continue;
            }

            if index == 1 {
                stream = list.pop_front().unwrap();
            } else {
                let mut split = list.split_off(index - 1);
                stream = split.pop_front().unwrap();
                list.append(&mut split);
            }
        } // End Mutex lock

        // Move semantics are a little weird right here, so we must use clones of clones
        // in order to make the compiler happy. This will be fixed once FnBox trait is
        // released into stable channel.
        if (event.events & READ_EVENT) > 0 {
            trace!("event was read event");
            let t_handler = handler.clone();
            let stream_list = streams.clone();
            pool.run(move || {
                handle_read_event(epfd, stream.clone(), stream_list.clone(),
                    t_handler.clone());
            });
        } else if (event.events & DROP_EVENT) > 0 {
            trace!("event was drop event");
            let t_handler = handler.clone();
            let stream_list = streams.clone();
            pool.run(move || {
                handle_drop_event(epfd, stream.clone(), stream_list.clone(), t_handler.clone());
            });
        } else { warn!("event was unknown: {}", event.data); }
    }
}

fn handle_read_event(epfd: RawFd,
                     stream: Nbstream,
                     streams: StreamList,
                     handler: SafeHandler) {
    trace!("handle read event");
    let mut stream = stream;
    let stream_fd = stream.as_raw_fd();
    match stream.recv() {
        Ok(_) => {
            let rx_queue = stream.drain_rx_queue();
            let stream_clone = stream.clone();
            { // Begin Mutex lock
                let mut guard = match streams.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("StreamList Mutex failed, using anyway...");
                        poisoned.into_inner()
                    }
                };
                let stream_list = guard.deref_mut();
                stream_list.push_back(stream);
            } // End Mutex lock

            for payload in rx_queue.iter() {
                let buf_len = payload.len();
                { // EventHandler Mutex locked
                    let mut guard = match handler.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => {
                            warn!("EventHandler Mutex failed, using anyway...");
                            poisoned.into_inner()
                        }
                    };
                    let e_handler = guard.deref_mut();
                    e_handler.on_data_received(stream_clone.clone(), payload.clone());
                } // EventHandler Mutex unlocked

                trace!("event handler finished");

                // Collect some stats
                stats::msg_recv();
                stats::bytes_recv(buf_len);

                trace!("stats collected");
            }
        }
        Err(e) => {
            error!("During read: {}", e);
            epoll_remove_fd(epfd, stream.as_raw_fd());
            close_fd(stream.as_raw_fd());
            let mut guard = match handler.lock() {
                Ok(g) => g,
                Err(p) => {
                    warn!("EventHandler Mutex failed, using anyway...");
                    p.into_inner()
                }
            };
            let e_handler = guard.deref_mut();
            e_handler.on_stream_closed(stream.id());
            return;
        }
    };

    // Re-enable stream in epoll watch list
    let mut event = EpollEvent {
        data: stream_fd as u64,
        events: EVENTS
    };
    let result = epoll::ctl(epfd, ctl_op::MOD, stream_fd, &mut event);
    if result.is_ok() {
        trace!("stream re-enabled in epoll successfully");
    } else {
        error!("During epoll::ctl: {}", result.unwrap_err());
    }
}

fn handle_drop_event(epfd: RawFd,
                     stream: Nbstream,
                     streams: StreamList,
                     handler: SafeHandler) {
    trace!("handle drop event");

    let stream_id = stream.id();
    let stream_fd = stream.as_raw_fd();

    epoll_remove_fd(epfd, stream_fd);
    remove_socket_from_list(stream_id.clone(), streams);
    close_fd(stream_fd);

    let mut guard = match handler.lock() {
        Ok(g) => g,
        Err(p) => {
            warn!("EventHandler Mutex failed, using anyway...");
            p.into_inner()
        }
    };
    let e_handler = guard.deref_mut();
    e_handler.on_stream_closed(stream_id);
}

fn epoll_remove_fd(epfd: RawFd, fd: RawFd) {
    debug!("removing fd from epoll");

    // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
    // a non-null pointer in event, even though this argument is ignored.
    // Since Linux 2.6.9, event can be specified as NULL when using
    // EPOLL_CTL_DEL.  Applications that need to be portable to kernels
    // before 2.6.9 should specify a non-null pointer in event.
    let _ = epoll::ctl(epfd, ctl_op::DEL, fd, &mut EpollEvent {
        data: 0 as u64,
        events: 0 as u32
    }).map_err(|e| {
        warn!("Epoll CtrlError during del: {}", e)
    });
}

/// Removes stream with id from master list
fn remove_socket_from_list(id: String, streams: StreamList) {
    debug!("removing stream from master list");

    let mut guard = match streams.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("StreamList Mutex was poisoned, using anyway");
            poisoned.into_inner()
        }
    };
    let list = guard.deref_mut();

    let mut found = false;
    let mut index = 1usize;
    for s in list.iter() {
        if s.id() == id {
            found = true;
            break;
        }
        index += 1;
    }

    if !found { return; }

    if index == 1 {
        list.pop_front();
    } else {
        let mut split = list.split_off(index - 1);
        split.pop_front();
        list.append(&mut split);
    }

    stats::conn_lost();

    trace!("socket removed from list");
}

fn close_fd(fd: RawFd) {
    unsafe {
        let result = libc::close(fd);
        if result < 0 {
            error!("Error closing fd: {}", Error::from_raw_os_error(result as i32));
        }
    }
}
