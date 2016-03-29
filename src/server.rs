// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::io::Error;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::net::{TcpStream, TcpListener};
use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};

use libc;
use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use threadpool::ThreadPool;
use openssl::ssl::{SslStream, SslContext};
use ss::nonblocking::plain::Plain;
use ss::nonblocking::secure::Secure;
use ss::{Socket, Stream, SRecv, SSend, TcpOptions, SocketOptions};
use simple_slab::Slab;

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
    let slab = Arc::new(Mutex::new(Slab::<Stream>::new(100000)));

    // Resource pool
    let mut tp = ThreadPool::new(config.workers);
    unsafe {
        thread_pool = &mut tp;
    }

    // Wrap our event handler into something that can be safely shared
    // between threads.
    let e_handler = Handler(Box::into_raw(handler));

    // Create an epoll instance
    let epoll_create_result = epoll::create1(0);
    if epoll_create_result.is_err() {
        error!("Unable to create epoll instance: {}", epoll_create_result.unwrap_err());
        return;
    }

    // Epoll instance
    let epfd = epoll_create_result.unwrap();

    // Epoll wait thread
    let epfd_clone = epfd.clone();
    let slab_clone = slab.clone();
    thread::Builder::new()
        .name("Epoll Wait".to_string())
        .spawn(move || {
            event_loop(epfd_clone, slab_clone, e_handler);
        })
        .unwrap();

    // New connection thread
    let epfd_clone = epfd.clone();
    let slab_clone = slab.clone();
    let prox = thread::Builder::new()
        .name("TCP Incoming Listener".to_string())
        .spawn(move || {
           listen(config, epfd_clone, slab_clone);
        })
        .unwrap();

    // Stay alive forever, or at least we hope
    let _ = prox.join();
}

/// Incoming connection listening thread
fn listen(config: Config, epfd: RawFd, slab_mutex: SlabMutex) {
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
            Ok(tcp_stream) => handle_new_connection(tcp_stream, &config, epfd, slab_mutex.clone()),
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

    { // Temporarily wrap in socket to use setsockopt easier
        let mut socket = Socket::new(server_fd);
        let reuseaddr_result = socket.set_reuseaddr(true);
        if reuseaddr_result.is_err() {
            return Err(reuseaddr_result.unwrap_err());
        }
    } // Scope added to make it easier to reason about temporary "cast"

    Ok(listener)
}

fn setup_ssl_context(config: &Config) {
    unsafe {
        ssl_context = Box::into_raw(Box::new(config.ssl.clone().unwrap()));
    }
}

fn handle_new_connection(tcp_stream: TcpStream, config: &Config, epfd: RawFd, slab_mutex: SlabMutex) {
    // Update our total opened file descriptors
    stats::fd_opened();

    let fd = tcp_stream.into_raw_fd();
    let socket = Socket::new(fd);

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

    // Temporarily wrap fd as Socket to make setting various options easier.
    // Options are set _after_ the stream is configured for a less error prone
    // OpenSSL handshake process.
    {
        let mut opt_socket = Socket::new(fd);
        let opt_set_result = setup_new_socket(&mut opt_socket);
        if opt_set_result.is_err() {
            close_fd(fd);
            return;
        }
    }

    // Add stream to our server
    add_stream_to_master_list(stream, slab_mutex.clone());
    add_fd_to_epoll(epfd, fd, slab_mutex.clone());

    // Updates our stats
    stats::conn_recv();
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
fn event_loop(epfd: RawFd, slab_mutex: SlabMutex, handler: Handler) {
    let mut events = Vec::<EpollEvent>::with_capacity(100);
    unsafe {
        events.set_len(100);
    }

    loop {
        match epoll::wait(epfd, &mut events[..], -1) {
            Ok(num_events) => {
                for x in 0..num_events as usize {
                    handle_epoll_event(epfd, &events[x], slab_mutex.clone(), handler.clone());
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
fn handle_epoll_event(epfd: RawFd, event: &EpollEvent, slab_mutex: SlabMutex, handler: Handler) {
    const READ_EVENT: u32 = event_type::EPOLLIN;

    let fd = event.data as RawFd;
    let find_result = try_find_stream_from_fd(slab_mutex.clone(), fd);
    if find_result.is_err() {
        remove_fd_from_epoll(epfd, fd);
        close_fd(fd);
    }

    let stream = find_result.unwrap();
    let read_event = (event.events & READ_EVENT) > 0;
    if read_event {
        handle_read_event(epfd, stream, slab_mutex, handler);
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

fn try_find_stream_from_fd(slab_mutex: SlabMutex, fd: RawFd) -> Result<Stream, ()> {
    let mut guard = match slab_mutex.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };
    let slab = guard.deref_mut();

    let mut offset = 0;
    let mut found = false;
    for x in 0..slab.len() {
        match slab[x] {
            Some(ref stream) => {
                if stream.as_raw_fd() == fd {
                    offset = x;
                    found = true;
                    break;
                }
            }
            None => { }
        }
    }

    if found {
        Ok(slab.remove(offset).unwrap())
    } else {
        warn!("Unable to locate fd: {} in SlabMutex", fd);
        Err(())
    }
}

/// Reads all available data on the stream.
///
/// If a complete message(s) is available, each message will be routed through the
/// resource pool.
///
/// If an error occurs during the read, the stream is dropped from the server.
fn handle_read_event(epfd: RawFd, stream: Stream, slab_mutex: SlabMutex, handler: Handler) {
    let mut stream = stream;
    let fd = stream.as_raw_fd();

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
        let sm = slab_mutex.clone();
        let h = handler.clone();

        if msg_is_stats_request(&msg) {
            handle_stats_request(&msg, epfd, s, sm);
        } else {
            unsafe {
                (*thread_pool).execute(move || {
                    let Handler(ptr) = h;
                    (*ptr).on_data_received(s, msg);
                });
            }
        }
    }

    add_stream_to_master_list(stream, slab_mutex);
}

#[inline]
fn msg_is_stats_request(msg: &[u8]) -> bool {
    if msg.len() == 6 && msg[0] == 0x04 && msg[1] == 0x04 {
        true
    } else {
        false
    }
}

fn handle_stats_request(buf: &[u8], epfd: RawFd, stream: Stream, slab_mutex: SlabMutex) {
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

    add_stream_to_master_list(stream_clone, slab_mutex);
}

/// Inserts the stream back into the master list of streams
fn add_stream_to_master_list(stream: Stream, slab_mutex: SlabMutex) {
    let mut guard = match slab_mutex.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };

    let slab = guard.deref_mut();
    slab.insert(stream);
}

/// Adds a new fd to the epoll instance
fn add_fd_to_epoll(epfd: RawFd, fd: RawFd, slab_mutex: SlabMutex) {
    let result = epoll::ctl(epfd,
                            ctl_op::ADD,
                            fd,
                            &mut EpollEvent {
                                data: fd as u64,
                                events: EVENTS,
                            });

    if result.is_err() {
        error!("epoll::CtrlError during add: {}", result.unwrap_err());

        remove_fd_from_list(fd, slab_mutex.clone());
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
fn remove_fd_from_list(fd: RawFd, slab_mutex: SlabMutex) {
    { // Mutex lock
        let mut guard = match slab_mutex.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };
        let slab = guard.deref_mut();

        let mut offset = 0;
        let mut found = false;
        for x in 0..slab.len() {
            match slab[x] {
                Some(ref stream) => {
                    if stream.as_raw_fd() == fd {
                        offset = x;
                        found = true;
                        break;
                    }
                }
                None => { }
            };
        }

        if !found {
            warn!("fd: {} not found in list when attempting removal", fd);
            return;
        }

        slab.remove(offset);
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
