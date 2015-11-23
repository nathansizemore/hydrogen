// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::{ptr, mem, thread};
use std::ffi::CString;
use std::io::{Error, ErrorKind};
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

use libc;
use epoll;
use epoll::util::*;
use epoll::EpollEvent;
use stream::socket::Socket;
use stream::nbstream::Nbstream;
use stats;
use types::*;
use resources::ResourcePool;
use libc::{c_int, c_void, c_char, c_ushort};
use config::Config;


extern "C" {
    fn shim_inet_pton(af: c_int, src: *const c_char, dst: *mut c_void) -> c_int;
    fn shim_htons(hostshort: c_ushort) -> c_ushort;
}

// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLIN      - Available for read
// EPOLLRDHUP   - Connection has been closed
// EPOLLONESHOT - After an event has been received, and reported, do not track
//                further changes until explicitly told to do so.
// EPOLLERR     - Some error occurred
// EPOLLHUP     - Hang up happened
const EVENTS: u32 = (
    event_type::EPOLLET | // Set fd to EdgeTrigger mode
    event_type::EPOLLIN |
    event_type::EPOLLRDHUP |
    event_type::EPOLLONESHOT |
    event_type::EPOLLERR |
    event_type::EPOLLHUP);

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
    let cfg_clone = config.clone();
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
        let cstr_addr = CString::new(config.addr).unwrap();
        let mut addr_buf = libc::malloc(mem::size_of::<libc::in_addr>());
        let server_addr = libc::sockaddr_in {
            sin_family: libc::AF_INET as u16,
            sin_port: shim_htons(config.port),
            sin_addr: libc::in_addr {
                s_addr: shim_inet_pton(libc::AF_INET, cstr_addr.as_ptr(), addr_buf) as u32
            },
            sin_zero: [0u8; 8]
        };

        let server_fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        let serv_addr: *const libc::sockaddr = mem::transmute(&server_addr);
        libc::bind(server_fd, serv_addr, mem::size_of::<libc::sockaddr_in>() as u32);
        libc::listen(server_fd, 100);

        loop {
            let client_fd = libc::accept(server_fd,
                ptr::null_mut() as *mut libc::sockaddr, ptr::null_mut() as *mut u32);
            let socket = Socket {
                fd: client_fd
            };
            let stream = Nbstream::new(socket).unwrap();

            let stream_ptr;
            { // Begin Mutex lock
                let mut guard = match streams.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("StreamList Mutex failed, using anyway...");
                        poisoned.into_inner()
                    }
                };
                let stream_list = guard.deref_mut();
                stream_list.push_back(stream.clone());
                stream_ptr = stream_list.back_mut().unwrap() as *mut Nbstream;
            } // End Mutex lock

            // Add to epoll
            let add_res = epoll::ctl(epfd, ctl_op::ADD, stream.as_raw_fd(), &mut EpollEvent {
                data: (stream_ptr as usize) as u64,
                events: EVENTS
            });
            if add_res.is_err() {
                error!("CtrlError during add: {}", add_res.unwrap_err());
            } else {
                stats::conn_recv();
            }
        }

        libc::free(addr_buf);
    }
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
                       streams: StreamList,
                       handler: SafeHandler) {
    // Types of events we care about
    const READ_EVENT: u32 = event_type::EPOLLIN;
    const DROP_EVENT: u32 = event_type::EPOLLRDHUP | event_type::EPOLLHUP | event_type::EPOLLERR;

    // Thread pool
    let mut pool = ResourcePool::new();

    for event in rx.iter() {
        trace!("received epoll event");

        // Move semantics are a little weird right here, so we must use clones of clones
        // in order to make the compiler happy. This will be fixed once FnBox trait is
        // released into stable channel.
        let stream_ptr = event.data as *mut Nbstream;
        if (event.events & DROP_EVENT) > 0 {
            trace!("event was drop event");
            let t_handler = handler.clone();
            let stream_list = streams.clone();
            let stream = unsafe { (*stream_ptr).clone() };
            pool.run(move || {
                handle_drop_event(epfd, stream.clone(), stream_list.clone(), t_handler.clone());
            });
        } else if (event.events & READ_EVENT) > 0 {
            trace!("event was read event");
            let t_handler = handler.clone();
            let stream_list = streams.clone();
            let stream = unsafe { (*stream_ptr).clone() };
            let ptr_addr = stream_ptr as usize;
            pool.run(move || {
                handle_read_event(epfd, stream.clone(), stream_list.clone(),
                    t_handler.clone(), ptr_addr);
            });
        } else { warn!("event was unknown: {}", event.data); continue; }
    }
}

fn handle_read_event(epfd: RawFd,
                     stream: Nbstream,
                     streams: StreamList,
                     handler: SafeHandler,
                     ptr_addr: usize) {
    let mut stream = stream;
    match stream.recv() {
        Ok(buf) => {
            let buf_len = buf.len();
            { // EventHandler Mutex locked
                let mut guard = match handler.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        warn!("EventHandler Mutex failed, using anyway...");
                        poisoned.into_inner()
                    }
                };
                let e_handler = guard.deref_mut();
                e_handler.on_data_received(stream.clone(), buf);
            } // EventHandler Mutex unlocked

            // Collect some stats
            stats::msg_recv();
            stats::bytes_recv(buf_len);
        }
        Err(e) => {
            if e.kind() != ErrorKind::WouldBlock {
                handle_drop_event(epfd, stream, streams, handler);
                return;
            }
        }
    };

    // Re-enable stream in epoll watch list
    let _ = epoll::ctl(epfd, ctl_op::MOD, stream.as_raw_fd(), &mut EpollEvent {
        data: ptr_addr as u64,
        events: EVENTS
    }).map_err(|e| {
        error!("During epoll::ctl: {}", e);
    });
}

fn handle_drop_event(epfd: RawFd,
                     stream: Nbstream,
                     streams: StreamList,
                     handler: SafeHandler) {
    epoll_remove_fd(epfd, stream.as_raw_fd());
    remove_socket_from_list(stream.id(), streams);
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
}

fn epoll_remove_fd(epfd: RawFd, fd: RawFd) {
    debug!("removing fd from epoll");

    // In kernel versions before 2.6.9, the EPOLL_CTL_DEL operation required
    // a non-null pointer in event, even though this argument is ignored.
    // Since Linux 2.6.9, event can be specified as NULL when using
    // EPOLL_CTL_DEL.  Applications that need to be portable to kernels
    // before 2.6.9 should specify a non-null pointer in event.
    epoll::ctl(epfd, ctl_op::DEL, fd, &mut EpollEvent {
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
