// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::io::Error;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::{mem, thread};
use std::net::{TcpStream, TcpListener};
use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};

use libc;
use libc::{c_int, c_void};
use slab::Slab;
use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use errno::errno;
use threadpool::ThreadPool;
use openssl::ssl::{SslStream, SslContext};
use ss::nonblocking::plain::Plain;
use ss::nonblocking::secure::Secure;
use ss::{Socket, Stream, SRecv, SSend, TcpOptions, SocketOptions};

use stats;
use types::*;
use config::Config;


// We need to be able to access our resource pool from several methods
static mut thread_pool: *mut ThreadPool = 0 as *mut ThreadPool;

// Global SslContext
static mut ssl_context: *mut SslContext = 0 as *mut SslContext;

// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLET  - Fd is in EdgeTriggered mode (notification on state changes)
// EPOLLIN  - Data is available in kerndl buffer
const EVENTS: u32 = event_type::EPOLLET | event_type::EPOLLIN;


/// Starts the epoll wait and incoming connection listener threads.
pub fn begin(config: Config, handler: Box<EventHandler>) {
    // Master socket list
    let sockets = Arc::new(Mutex::new(Slab::<Stream, usize>::new(100000)));

    // Resource pool
    let mut tp = ThreadPool::new(config.workers);
    unsafe {
        thread_pool = &mut tp;
    }

    // Wrap our event handler into something that can be safely shared
    // between threads.
    let e_handler = Handler(Box::into_raw(handler));

    // Epoll instance
    let result = epoll::create1(0);
    if result.is_err() {
        let err = result.unwrap_err();
        error!("Unable to create epoll instance: {}", err);
        return;
    }
    let epfd = result.unwrap();

    // Epoll wait thread
    let epfd2 = epfd.clone();
    let streams2 = sockets.clone();
    thread::Builder::new()
        .name("Epoll Wait".to_string())
        .spawn(move || {
            event_loop(epfd2, streams2, e_handler);
        })
        .unwrap();

    // New connection thread
    let epfd3 = epfd.clone();
    let streams3 = sockets.clone();
    let prox = thread::Builder::new()
        .name("TCP Incoming Listener".to_string())
        .spawn(move || {
           listen(config, epfd3, streams3);
        })
        .unwrap();

    // Stay alive forever, or at least we hope
    let _ = prox.join();
}

/// Incoming connection listening thread
fn listen(config: Config, epfd: RawFd, streams: StreamList) {
    // Setup server and listening port
    let listener_result = try_setup_tcp_listener(&config);
    if listener_result.is_err() {
        error!("Setting up server: {}", listener_result.unwrap_err());
        return;
    }

    // If we're using SSL, setup our context reference
    if config.ssl.is_some() {
        setup_ssl_context(&config);
    }

    // Begin listening for new connections
    let listener = listener_result.unwrap();
    for accept_result in listener.incoming() {
        match accept_result {
            Ok(tcp_stream) => handle_new_connection(tcp_stream, &config, epfd, streams.clone()),
            Err(e) => error!("Accepting connection: {}", e)
        }
    }

    drop(listener);
}

fn try_setup_tcp_listener(config: &Config) -> Result<TcpListener, Error> {
    let create_result = TcpListener::bind((&config.addr[..], config.port));
    if create_result.is_err() {
        return create_result;
    }

    let listener = create_result.unwrap();
    let server_fd = listener.as_raw_fd();

    // Set the SO_REUSEADDR so that restarts after crashes do not take 5min to unbind
    // the initial port
    unsafe {
        let optval: c_int = 1;
        let opt_result = libc::setsockopt(server_fd,
                                          libc::SOL_SOCKET,
                                          libc::SO_REUSEADDR,
                                          &optval as *const _ as *const c_void,
                                          mem::size_of::<c_int>() as u32);
        if opt_result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32));
        }
    }

    Ok(listener)
}

fn setup_ssl_context(config: &Config) {
    unsafe {
        ssl_context = Box::into_raw(Box::new(config.ssl.clone().unwrap()));
    }
}

fn handle_new_connection(tcp_stream: TcpStream, config: &Config, epfd: RawFd, streams: StreamList) {
    // Update our total opened file descriptors
    stats::fd_opened();

    // Create new socket
    let socket = Socket::new(tcp_stream.into_raw_fd());

    // Socket's fd for option setting after connection has been established
    let fd = socket.as_raw_fd();

    // Setup our stream
    let stream = match config.ssl {
        Some(_) => {
            let ssl_result = unsafe { SslStream::accept(&(*ssl_context), socket) };
            match ssl_result {
                Ok(ssl_stream) => {
                    let secure_stream = Secure::new(ssl_stream);
                    Stream::new(Box::new(secure_stream))
                }
                Err(ssl_error) => {
                    error!("Creating SslStream: {}", ssl_error);
                    close_fd(fd);
                    return;
                }
            }
        }
        None => {
            let plain_text = Plain::new(socket);
            Stream::new(Box::new(plain_text))
        }
    };

    // Setup various socket options
    let mut opt_socket = Socket::new(fd);
    let result = setup_new_socket(&mut opt_socket);
    if result.is_err() {
        close_fd(fd);
        return;
    }

    // Add stream to our server
    let fd = stream.as_raw_fd();
    add_stream_to_master_list(stream, streams.clone());
    add_fd_to_epoll(epfd, fd, streams.clone());
}

/// Sets up various socket options
fn setup_new_socket(socket: &mut Socket) -> Result<(), ()> {
    let result = socket.set_nonblocking();
    if result.is_err() {
        error!("Setting fd to nonblocking: {}", result.unwrap_err());
        return Err(());
    }

    let result = socket.set_tcp_nodelay(true);
    if result.is_err() {
        error!("Setting tcp_nodelay: {}", result.unwrap_err());
        return Err(());
    }

    let result = socket.set_keepalive(true);
    if result.is_err() {
        error!("Setting tcp_keepalive: {}", result.unwrap_err());
        return Err(());
    }

    Ok(())
}

/// Event loop for handling all epoll events
fn event_loop(epfd: RawFd, streams: StreamList, handler: Handler) {
    let mut events = Vec::<EpollEvent>::with_capacity(100);
    unsafe {
        events.set_len(100);
    }

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
fn handle_epoll_event(epfd: RawFd, event: &EpollEvent, streams: StreamList, handler: Handler) {
    const READ_EVENT: u32 = event_type::EPOLLIN;

    let fd = event.data as RawFd;
    let find_result = try_find_stream_from_fd(streams.clone(), fd);
    if find_result.is_err() {
        remove_fd_from_epoll(epfd, fd);
        close_fd(fd);
    }

    let stream = find_result.unwrap();
    let read_event = if (event.events & READ_EVENT) > 0 {
        true
    } else {
        false
    };

    if read_event {
        handle_read_event(epfd, stream, streams, handler);
    } else {
        remove_fd_from_epoll(epfd, fd);
        close_fd(fd);

        unsafe {
            (*thread_pool).execute(move || {
                let Handler(ptr) = handler;
                (*ptr).on_stream_closed(fd);
            });
        }
    }
}

fn try_find_stream_from_fd(streams: StreamList, fd: RawFd) -> Result<Stream, ()> {
    let mut guard = match streams.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };
    let list = guard.deref_mut();

    let mut found = false;
    let mut index = 0;
    for x in 0..list.count() {
        match list.get(x) {
            Some(stream) => {
                if stream.as_raw_fd() == fd {
                    found = true;
                    index = x;
                    break;
                }
            }
            None => { }
        };
    }

    if found {
        Ok(list.remove(index).unwrap())
    } else {
        Err(())
    }
}

/// Reads all available data on the stream.
///
/// If a complete message(s) is available, each message will be routed through the
/// resource pool.
///
/// If an error occurs during the read, the stream is dropped from the server.
fn handle_read_event(epfd: RawFd, stream: Stream, streams: StreamList, handler: Handler) {
    let fd = stream.as_raw_fd();

    let mut stream = stream;
    let recv_result = stream.recv();
    if recv_result.is_err() {
        remove_fd_from_epoll(epfd, fd);
        close_fd(fd);

        unsafe {
            (*thread_pool).execute(move || {
                let Handler(ptr) = handler;
                (*ptr).on_stream_closed(fd);
            });
        }
        return;
    }

    let mut msg_queue = stream.drain_rx_queue();
    for msg in msg_queue.drain(..) {
        let s = stream.clone();
        let sl = streams.clone();
        let h = handler.clone();

        if msg_is_stats_request(&msg) {
            handle_stats_request(&msg, epfd, s, sl);
        } else {
            unsafe {
                (*thread_pool).execute(move || {
                    let Handler(ptr) = h;
                    (*ptr).on_data_received(s, msg);
                });
            }
        }
    }

    add_stream_to_master_list(stream, streams);
}

#[inline]
fn msg_is_stats_request(msg: &[u8]) -> bool {
    if msg.len() == 6 && msg[0] == 0x04 && msg[1] == 0x04 {
        true
    } else {
        false
    }
}

fn handle_stats_request(buf: &[u8], epfd: RawFd, stream: Stream, streams: StreamList) {
    trace!("handle_stats_request");
    let stream_clone = stream.clone();
    let u8ptr: *const u8 = &buf[2] as *const _;
    let f32ptr: *const f32 = u8ptr as *const _;
    unsafe {
        let sec = *f32ptr;
        (*thread_pool).execute(move || {
            let stats_result = stats::as_serialized_buffer(sec);
            if stats_result.is_err() {
                error!("Retrieving stats");
                return;
            }

            let stats_buffer = stats_result.unwrap();

            let mut stream = stream;
            let send_result = stream.send(&stats_buffer[..]);
            if send_result.is_err() {
                error!("Writing to stream: {}", send_result.unwrap_err());
                let fd = stream.as_raw_fd();
                remove_fd_from_epoll(epfd, fd);
                close_fd(fd);
            }
        });
    }

    add_stream_to_master_list(stream_clone, streams);
}

/// Inserts the stream back into the master list of streams
fn add_stream_to_master_list(stream: Stream, streams: StreamList) {
    { // Mutex lock
        let mut guard = match streams.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };

        let list = guard.deref_mut();

        // Allocate if we need more slab space
        if !list.has_remaining() {
            let extra_capacity = list.count();
            list.grow(extra_capacity);
        }

        let _ = list.insert(stream);
    } // Mutex unlock

    stats::conn_recv();
}

/// Adds a new fd to the epoll instance
fn add_fd_to_epoll(epfd: RawFd, fd: RawFd, streams: StreamList) {
    let result = epoll::ctl(epfd,
                            ctl_op::ADD,
                            fd,
                            &mut EpollEvent {
                                data: fd as u64,
                                events: EVENTS,
                            });

    if result.is_err() {
        error!("epoll::CtrlError during add: {}", result.unwrap_err());

        remove_fd_from_list(fd, streams.clone());
        close_fd(fd);
    }
}

/// Removes a fd from the epoll instance
fn remove_fd_from_epoll(epfd: RawFd, fd: RawFd) {
    // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
    // a non-null pointer in event, even though this argument is ignored.
    // Since Linux 2.6.9, event can be specified as NULL when using
    // EPOLL_CTL_DEL. We'll be as backwards compatible as possible.
    let _ = epoll::ctl(epfd,
                       ctl_op::DEL,
                       fd,
                       &mut EpollEvent {
                           data: 0 as u64,
                           events: 0 as u32,
                       })
                .map_err(|e| warn!("Epoll CtrlError during del: {}", e));
}

/// Removes stream with fd from master list
fn remove_fd_from_list(fd: RawFd, streams: StreamList) {
    { // Mutex lock
        let mut guard = match streams.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };
        let list = guard.deref_mut();

        let mut found = false;
        let mut index = 0;
        for x in 0..list.count() {
            match list.get(x) {
                Some(stream) => {
                    if stream.as_raw_fd() == fd {
                        found = true;
                        index = x;
                        break;
                    }
                }
                None => { }
            };
        }

        if !found {
            return;
        }

        list.remove(index);
    } // Mutex unlock

    stats::conn_lost();
}

/// Closes a fd with the kernel
fn close_fd(fd: RawFd) {
    unsafe {
        let result = libc::close(fd);
        if result < 0 {
            error!("Error closing fd: {}",
                   Error::from_raw_os_error(result as i32));
            return;
        }
    }
    stats::fd_closed();
}
