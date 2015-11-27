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
    fn shim_inaddr_any() -> c_ulong;
}

// 100% memory safety is for wimps anyway
static mut pool: *mut ResourcePool = 0 as *mut ResourcePool;

// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLET  - Fd is in EdgeTriggered mode (notification on state changes)
// EPOLLIN  - Data is available in kerndl buffer
const EVENTS: u32 = event_type::EPOLLET | event_type::EPOLLIN;


pub fn begin<T>(config: Config, handler: Box<T>) where
    T: EventHandler + Send + Sync + 'static {
    // Master socket list
    let sockets = Arc::new(Mutex::new(LinkedList::<Nbstream>::new()));

    // Resource pool
    let mut rp = ResourcePool::new();
    unsafe { pool = &mut rp; }

    // Wrap our event handler into something that can be safely shared
    // between threads.
    let e_handler = Arc::new(Mutex::new(*handler));

    // Epoll instance
    let result = epoll::create1(0);
    if result.is_err() {
        let err = result.unwrap_err();
        error!("Unable to create epoll instance: {}", err);
        panic!()
    }
    let epfd = result.unwrap();

    // Epoll wait thread
    let epfd2 = epfd.clone();
    let streams2 = sockets.clone();
    thread::Builder::new()
        .name("Epoll Wait".to_string())
        .spawn(move || {
            event_loop(epfd2, streams2, e_handler);
        }).unwrap();

    // New connection thread
    let epfd3 = epfd.clone();
    let streams3 = sockets.clone();
    let prox = thread::Builder::new()
        .name("TCP Incoming Listener".to_string())
        .spawn(move || {
            listen(config, epfd3, streams3);
        }).unwrap();

    // Stay alive forever, or at least we hope
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
            return;
        }

        loop {
            // Accept new client
            let result = libc::accept(server_fd,
                ptr::null_mut() as *mut libc::sockaddr, ptr::null_mut() as *mut u32);
            if result < 0 {
                error!("Accepting new connection: {}", Error::from_raw_os_error(result as i32));
            }

            // Create new stream
            let _ = Nbstream::new(Socket {
                fd: result
            }).map(|stream| {
                let fd = stream.as_raw_fd();
                add_stream_to_master_list(stream, streams.clone());
                add_to_epoll(epfd, fd, streams.clone());
            }).map_err(|e| {
                // If we ever fail here, it is safe to assume everything else has gone to
                // shit. No way to clean it up, so we'll just leave. The main process thread has
                // joined to this tread, so panic-ing will cause our parent process to end nicely.
                error!("Creating Nbstream: {}", e);
                panic!()
            });
        }
    }
}

fn event_loop(epfd: RawFd, streams: StreamList, handler: SafeHandler) {
    let mut events = Vec::<EpollEvent>::with_capacity(100);
    unsafe { events.set_len(100); }

    loop {
        match epoll::wait(epfd, &mut events[..], -1) {
            Ok(num_events) => {
                for x in 0..num_events as usize {
                    handle_epoll_event(epfd, &events[x], streams.clone(), handler.clone());
                }
            }
            Err(e) => {
                error!("Error on epoll::wait(): {}", e);
                panic!()
            }
        };
    }
}

fn handle_epoll_event(epfd: RawFd,
                      event: &EpollEvent,
                      streams: StreamList,
                      handler: SafeHandler) {
    const READ_EVENT: u32 = event_type::EPOLLIN;

    // Locate the stream the event was for
    let mut stream;
    { // Mutex lock
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
            trace!("event_fd: {} fd: {}", event.data, fd);
            if s.as_raw_fd() == event.data as RawFd {
                found = true;
                break;
            }
            index += 1;
        }

        if !found {
            error!("Could not find fd in stream list?");
            return;
        }

        if index == 1 {
            stream = list.pop_front().unwrap();
        } else {
            let mut split = list.split_off(index - 1);
            stream = split.pop_front().unwrap();
            list.append(&mut split);
        }
    } // Mutex unlock

    if (event.events & READ_EVENT) > 0 {
        trace!("event was read event");
        handle_read_event(epfd, &mut stream, streams.clone(), handler.clone()).map(|_| {
            add_stream_to_master_list(stream, streams.clone());
        });
    } else {
        trace!("event was drop event");
        handle_drop_event(epfd, &stream, streams.clone(), handler.clone());
    }
}



fn handle_read_event(epfd: RawFd,
                     stream: &mut Nbstream,
                     streams: StreamList,
                     handler: SafeHandler) -> Result<(), ()> {
    trace!("handle read event");

    match stream.recv() {
        Ok(_) => {
            let rx_queue = stream.drain_rx_queue();
            for payload in rx_queue.iter() {
                // Yes, this is terrible. But, move sematics are a little shitty right now until
                // Box<FnOnce> gets stabilized. Hopefully in 1.5?
                // TODO - Refactor once better function passing traits are available in stable.
                let buf_len = payload.len();
                let handler_cpy = handler.clone();
                let stream_cpy = stream.clone();
                let payload_cpy = payload.clone();
                unsafe {
                    (*pool).run(move || {
                        { // Mutex locked
                            let mut guard = match handler_cpy.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => {
                                    warn!("EventHandler Mutex failed, using anyway...");
                                    poisoned.into_inner()
                                }
                            };
                            let e_handler = guard.deref_mut();
                            e_handler.on_data_received(stream_cpy.clone(), payload_cpy.clone());
                        } // Mutex unlocked
                    });
                }

                // Collect some stats
                stats::msg_recv();
                stats::bytes_recv(buf_len);
            }
            Ok(())
        }
        Err(e) => {
            error!("During read: {}", e);
            remove_fd_from_epoll(epfd, stream.as_raw_fd());
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
            Err(())
        }
    }
}

fn handle_drop_event(epfd: RawFd,
                     stream: &Nbstream,
                     streams: StreamList,
                     handler: SafeHandler) {
    trace!("handle drop event");

    let stream_id = stream.id();
    let stream_fd = stream.as_raw_fd();

    remove_fd_from_epoll(epfd, stream_fd);
    remove_fd_from_list(stream_fd, streams);
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

fn add_stream_to_master_list(stream: Nbstream, streams: StreamList) {
    let mut guard = match streams.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("StreamList Mutex failed, using anyway...");
            poisoned.into_inner()
        }
    };
    let stream_list = guard.deref_mut();
    stream_list.push_back(stream);
}

fn add_to_epoll(epfd: RawFd, fd: RawFd, streams: StreamList) {
    let _ = epoll::ctl(epfd, ctl_op::ADD, fd, &mut EpollEvent {
        data: fd as u64,
        events: EVENTS
    }).map(|_| {
        trace!("New fd added to epoll");
        stats::conn_recv();
    }).map_err(|e| {
        error!("CtrlError during add: {}", e);
        remove_fd_from_list(fd, streams.clone());
        close_fd(fd);
    });
}

fn remove_fd_from_epoll(epfd: RawFd, fd: RawFd) {
    debug!("removing fd from epoll");

    // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
    // a non-null pointer in event, even though this argument is ignored.
    // Since Linux 2.6.9, event can be specified as NULL when using
    // EPOLL_CTL_DEL. We'll be as backwards compatible as possible.
    let _ = epoll::ctl(epfd, ctl_op::DEL, fd, &mut EpollEvent {
        data: 0 as u64,
        events: 0 as u32
    }).map_err(|e| {
        warn!("Epoll CtrlError during del: {}", e)
    });
}

/// Removes stream with id from master list
fn remove_fd_from_list(fd: RawFd, streams: StreamList) {
    trace!("removing fd: {} from master list", fd);

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
        if s.as_raw_fd() == fd {
            found = true;
            break;
        }
        index += 1;
    }

    if !found {
        trace!("fd: {} not found in list", fd);
        return;
    }

    if index == 1 {
        list.pop_front();
    } else {
        let mut split = list.split_off(index - 1);
        split.pop_front();
        list.append(&mut split);
    }
}

fn close_fd(fd: RawFd) {
    unsafe {
        let result = libc::close(fd);
        if result < 0 {
            error!("Error closing fd: {}", Error::from_raw_os_error(result as i32));
            return;
        }
    }
    stats::conn_lost();
}
