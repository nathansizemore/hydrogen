// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::io::Error;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::{ptr, mem, thread};
use std::os::unix::io::{RawFd, AsRawFd};
use std::collections::LinkedList;

use libc;
use errno::errno;
use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use stream::socket::Socket;
use stream::nbstream::Nbstream;
use stream::{Stream, HRecv, HSend, CloneHStream};
use stats;
use types::*;
use resources::ResourcePool;
use libc::{c_ushort, c_ulong, c_uint, c_int, c_void};
use config::Config;


extern "C" {
    fn shim_htons(hostshort: c_ushort) -> c_ushort;
    fn shim_htonl(addr: c_uint) -> c_uint;
    fn shim_inaddr_any() -> c_ulong;
}

// We need to be able to access our resource pool from several methods
static mut pool: *mut ResourcePool = 0 as *mut ResourcePool;

// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLET  - Fd is in EdgeTriggered mode (notification on state changes)
// EPOLLIN  - Data is available in kerndl buffer
const EVENTS: u32 = event_type::EPOLLET | event_type::EPOLLIN;

/// Starts the epoll wait and incoming connection listener threads.
pub fn begin(config: Config, handler: Box<EventHandler>) {
    // Master socket list
    let sockets = Arc::new(Mutex::new(LinkedList::<Stream>::new()));

    // Resource pool
    let mut rp = ResourcePool::new();
    unsafe { pool = &mut rp; }

    // Wrap our event handler into something that can be safely shared
    // between threads.
    let e_handler = Handler(Box::into_raw(handler));

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

/// Incoming connection listening thread
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

        // Create a server socket with the passed info for TCP/IP
        let server_fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);

        // Set the SO_REUSEADDR so that restarts after crashes do not take 5min to unbind
        // the initial port
        let optval: c_int = 1;
        let opt_result = libc::setsockopt(server_fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &optval as *const _ as *const c_void,
            mem::size_of::<c_int>() as u32);
        if opt_result < 0 {
            error!("Setting SO_REUSEADDR: {}", Error::from_raw_os_error(errno().0 as i32));
            return;
        }

        let serv_addr: *const libc::sockaddr = mem::transmute(&server_addr);
        let bind_result = libc::bind(server_fd, serv_addr,
            mem::size_of::<libc::sockaddr_in>() as u32);
        if bind_result < 0 {
            error!("Binding fd: {} - {}", server_fd, Error::from_raw_os_error(errno().0 as i32));
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
            }).map(|nbstream| {
                let stream = Stream {
                    inner: Box::new(nbstream)
                };
                let fd = stream.as_raw_fd();
                add_stream_to_master_list(stream, streams.clone());
                add_to_epoll(epfd, fd, streams.clone());
                stats::conn_recv();
            }).map_err(|e| {
                // If we ever fail here, it is safe to assume everything else has gone to
                // shit. No way to clean it up, so we'll just leave. The main process thread has
                // joined to this tread, so panic-ing will cause our parent process to end nicely.
                error!("Creating stream: {}", e);
                panic!()
            });
        }
    }
}

/// Event loop for handling all epoll events
fn event_loop(epfd: RawFd, streams: StreamList, handler: Handler) {
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

/// Finds the stream the epoll event is associated with and parses the event type
/// to hand off to specific handlers
fn handle_epoll_event(epfd: RawFd,
                      event: &EpollEvent,
                      streams: StreamList,
                      handler: Handler) {
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
            if s.as_raw_fd() == event.data as RawFd {
                found = true;
                break;
            }
            index += 1;
        }

        if !found {
            error!("Could not find fd in stream list? Removing from epoll and closing fd");

            let fd = event.data as RawFd;
            remove_fd_from_epoll(epfd, fd);
            close_fd(fd);
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
        let _ = handle_read_event(epfd, &mut stream, handler).map(|_| {
            add_stream_to_master_list(stream, streams.clone());
        });
    } else {
        trace!("event was drop event");

        let fd = stream.as_raw_fd();
        remove_fd_from_epoll(epfd, fd);
        close_fd(fd);

        let stream_fd = stream.as_raw_fd();
        unsafe {
            (*pool).run(move || {
                let Handler(ptr) = handler;
                (*ptr).on_stream_closed(stream_fd);
            });
        }
    }
}

/// Reads all available data on the stream.
///
/// If a complete message(s) is available, each message will be routed through the
/// resource pool.
///
/// If an error occurs during the read, the stream is dropped from the server.
fn handle_read_event(epfd: RawFd, stream: &mut Stream, handler: Handler) -> Result<(), ()> {
    trace!("handle read event");

    match stream.recv() {
        Ok(_) => {
            let mut rx_queue = stream.drain_rx_queue();
            for payload in rx_queue.iter_mut() {
                // Check if this is a request for stats
                if payload.len() == 6
                    && payload[0] == 0x04
                    && payload[1] == 0x04 {
                    let u8ptr: *const u8 = &payload[2] as *const _;
                    let f32ptr: *const f32 = u8ptr as *const _;
                    let sec = unsafe { *f32ptr };
                    let stream_cpy = stream.clone();
                    unsafe {
                        (*pool).run(move || {
                            let mut s = stream_cpy.clone();
                            let result = stats::as_serialized_buffer(sec);
                            if result.is_ok() {
                                let _ = s.send(&result.unwrap()[..]);
                            }
                        });
                    }
                    return Ok(())
                }

                // TODO - Refactor once better function passing traits are available in stable.
                let buf_len = payload.len();
                let handler_cpy = handler.clone();
                let stream_cpy = Stream {
                    inner: stream.inner.clone_h_stream()
                };
                let payload_cpy = payload.clone();
                unsafe {
                    (*pool).run(move || {
                        let Handler(ptr) = handler_cpy;
                        (*ptr).on_data_received(stream_cpy.clone(), payload_cpy.clone());
                    });
                }

                // Collect some stats
                stats::msg_recv();
                stats::bytes_recv(buf_len);
            }
            Ok(())
        }
        Err(e) => {
            error!("stream.fd: {} during read: {}", stream.as_raw_fd(), e);

            remove_fd_from_epoll(epfd, stream.as_raw_fd());
            close_fd(stream.as_raw_fd());

            let stream_fd = stream.as_raw_fd();
            unsafe {
                (*pool).run(move || {
                    let Handler(ptr) = handler;
                    (*ptr).on_stream_closed(stream_fd.clone());
                });
            }

            Err(())
        }
    }
}

/// Inserts the stream back into the master list of streams
fn add_stream_to_master_list(stream: Stream, streams: StreamList) {
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

/// Adds a new fd to the epoll instance
fn add_to_epoll(epfd: RawFd, fd: RawFd, streams: StreamList) {
    let result = epoll::ctl(epfd, ctl_op::ADD, fd, &mut EpollEvent {
        data: fd as u64,
        events: EVENTS
    });

    if result.is_ok() {
        trace!("New fd added to epoll");
        stats::conn_recv();
    } else {
        let e = result.unwrap_err();
        error!("CtrlError during add: {}", e);
        remove_fd_from_list(fd, streams.clone());
        close_fd(fd);
    }
}

/// Removes a fd from the epoll instance
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

/// Closes a fd with the kernel
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
