// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::io::{Error, ErrorKind};
use std::time::Duration;
use std::ops::DerefMut;
use std::cell::UnsafeCell;
use std::sync::{Arc, Mutex};
use std::net::{TcpStream, TcpListener};
use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};

use libc;
use errno::errno;
use threadpool::ThreadPool;
use simple_slab::Slab;

use types::*;
use config::Config;
use super::{Stream, Handler};


// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLIN          - Data is available in kernel buffer.
// EPOLLOUT         - Kernel's buffer has room to write to.
// EPOLLRDHUP       - Peer closed connection.
// EPOLLPRI         - Urgent data for read available.
// EPOLLET          - Register in EdgeTrigger mode.
// EPOLLONESHOT     - After an event is pulled out with epoll_wait(2) the associated
//                    file descriptor is internally disabled and no other events will
//                    be reported by the epoll interface.
const EVENTS_R: i32 = libc::EPOLLIN |
                    libc::EPOLLRDHUP |
                    libc::EPOLLPRI |
                    libc::EPOLLET |
                    libc::EPOLLONESHOT;
const EVENTS_RW: i32 = libc::EPOLLIN |
                    libc::EPOLLOUT |
                    libc::EPOLLRDHUP |
                    libc::EPOLLPRI |
                    libc::EPOLLET |
                    libc::EPOLLONESHOT;

// Maximum number of events returned from epoll_wait
const MAX_EVENTS: i32 = 100;


pub fn begin(handler: Box<Handler>, cfg: Config) {
    // Wrap handler in something we can share between threads
    let event_handler = EventHandler(Box::into_raw(handler));

    // Create our new connections slab
    let new_connection_slab = Arc::new(Mutex::new(Slab::<Connection>::new(10)));

    // Create our connection slab
    let mut_slab = MutSlab {
        inner: UnsafeCell::new(Slab::<Arc<Connection>>::new(cfg.pre_allocated))
    };
    let connection_slab = Arc::new(mut_slab);

    // Start the event loop
    let threads = cfg.max_threads;
    let eh_clone = event_handler.clone();
    let new_connections = new_connection_slab.clone();
    unsafe {
        thread::Builder::new()
            .name("Event Loop".to_string())
            .spawn(move || {
                event_loop(new_connections, connection_slab, eh_clone, threads)
            })
            .unwrap();
    }

    // Start the TcpListener loop
    let eh_clone = event_handler.clone();
    let listener_thread = unsafe {
        thread::Builder::new()
            .name("TcpListener Loop".to_string())
            .spawn(move || { listener_loop(cfg, new_connection_slab, eh_clone) })
            .unwrap()
    };
    let _ = listener_thread.join();

}

unsafe fn listener_loop(cfg: Config, new_connections: NewConnectionSlab, handler: EventHandler) {
    let listener_result = TcpListener::bind((&cfg.addr[..], cfg.port));
    if listener_result.is_err() {
        let err = listener_result.unwrap_err();
        error!("Creating TcpListener: {}", err);
        panic!();
    }

    let listener = listener_result.unwrap();
    setup_listener_options(&listener, handler.clone());

    for accept_attempt in listener.incoming() {
        match accept_attempt {
            Ok(tcp_stream) => handle_new_connection(tcp_stream, &new_connections, handler.clone()),
            Err(e) => error!("Accepting connection: {}", e)
        };
    }

    drop(listener);
}

unsafe fn setup_listener_options(listener: &TcpListener, handler: EventHandler) {
    let fd = listener.as_raw_fd();
    let EventHandler(handler_ptr) = handler;

    (*handler_ptr).on_server_created(fd);
}

unsafe fn handle_new_connection(tcp_stream: TcpStream,
                                new_connections: &NewConnectionSlab,
                                handler: EventHandler)
{
    // Take ownership of tcp_stream's underlying file descriptor
    let fd = tcp_stream.into_raw_fd();

    // Execute EventHandler's constructor
    let EventHandler(handler_ptr) = handler;
    let arc_stream = (*handler_ptr).on_new_connection(fd);

    // Create a connection structure
    let connection = Connection {
        fd: fd,
        err_mutex: Mutex::new(None),
        tx_mutex: Mutex::new(()),
        stream: arc_stream
    };

    // Insert it into the NewConnectionSlab
    let mut guard = match (*new_connections).lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };

    let slab = guard.deref_mut();
    slab.insert(connection);
}

/// Main event loop
unsafe fn event_loop(new_connections: NewConnectionSlab,
                     connection_slab: ConnectionSlab,
                     handler: EventHandler,
                     threads: usize)
{
    const MAX_WAIT: i32 = 1000; // Milliseconds

    // Attempt to create an epoll instance
    let result = libc::epoll_create(1);
    if result < 0 {
        let err = Error::from_raw_os_error(errno().0 as i32);
        error!("Creating epoll instance: {}", err);
        panic!();
    }

    // Epoll instance
    let epfd = result;

    // ThreadPool with user specified number of threads
    let thread_pool = ThreadPool::new(threads);

    // Start the I/O Sentinel
    let conn_slab_clone = connection_slab.clone();
    let t_pool_clone = thread_pool.clone();
    let handler_clone = handler.clone();
    thread::Builder::new()
        .name("I/O Sentinel".to_string())
        .spawn(move || { io_sentinel(conn_slab_clone, t_pool_clone, handler_clone) })
        .unwrap();

    // Our I/O queue for Connections needing various I/O operations.
    let arc_io_queue = Arc::new(Mutex::new(Vec::<IoPair>::with_capacity(MAX_EVENTS as usize)));

    // Scratch space for epoll returned events
    let mut event_buffer = Vec::<libc::epoll_event>::with_capacity(MAX_EVENTS as usize);
    event_buffer.set_len(MAX_EVENTS as usize);

    loop {
        // Remove any connections in an error'd state.
        remove_stale_connections(&connection_slab, &thread_pool, &handler);

        // Insert any newly received connections into the connection_slab
        insert_new_connections(&new_connections, &connection_slab, epfd);

        // Check for any new events
        let result = libc::epoll_wait(epfd, event_buffer.as_mut_ptr(), MAX_EVENTS, MAX_WAIT);
        if result < 0 {
            let err = Error::from_raw_os_error(errno().0 as i32);
            error!("During epoll_wait: {}", err);
            panic!();
        }

        let num_events = result as usize;
        update_io_events(&connection_slab, &arc_io_queue, &event_buffer[0..num_events]);
    }
}

/// Traverses through the connection slab and creates a list of connections that need dropped,
/// then traverses that list, drops them, and informs the handler of client drop.
unsafe fn remove_stale_connections(connection_slab: &ConnectionSlab,
                                   thread_pool: &ThreadPool,
                                   handler: &EventHandler)
{
    let slab_ptr = (*connection_slab).inner.get();
    let slab_len = (*slab_ptr).len() as isize;

    let mut x: isize = 0;
    while x < slab_len {
        let connection_opt = (*slab_ptr)[x as usize].as_ref();
        x += 1;

        if connection_opt.is_none() {
            continue;
        }

        let arc_connection = connection_opt.unwrap();

        let err_state;
        { // Mutex lock
            let guard = match arc_connection.err_mutex.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };
            err_state = (*guard).clone();
        } // Mutex unlock

        match err_state {
            Some(err) => {
                let arc_connection = (*slab_ptr).remove(x as usize).unwrap();

                // Inform kernel we're done
                close_connection(&arc_connection);

                // Inform the consumer connection is no longer valid
                let fd = (*arc_connection).fd;
                let handler_clone = (*handler).clone();
                thread_pool.execute(move || {
                    let EventHandler(ptr) = handler_clone;
                    (*ptr).on_connection_removed(fd, err);
                });

                x -= 1;
            }
            None => { }
        };
    }
}

/// Closes the connection's underlying file descriptor
unsafe fn close_connection(connection: &Arc<Connection>) {
    let fd = (*connection).fd;

    let result = libc::close(fd);
    if result < 0 {
        let err = Error::from_raw_os_error(errno().0 as i32);
        error!("Error closing fd: {}", err);
    }
}

/// Transfers Connections from the new_connections slab to the "main" connection_slab.
unsafe fn insert_new_connections(new_connections: &NewConnectionSlab,
                                 connection_slab: &ConnectionSlab,
                                 epfd: RawFd)
{
    let mut guard = match new_connections.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };

    let new_slab = guard.deref_mut();
    let num_connections = new_slab.len();
    let arc_main_slab = (*connection_slab).inner.get();
    for _ in 0..num_connections {
        let connection = new_slab.remove(0).unwrap();
        let arc_connection = Arc::new(connection);
        (*arc_main_slab).insert(arc_connection.clone());
        add_connection_to_epoll(epfd, arc_connection);
    }
}

/// Adds a new connection to the epoll interest list.
unsafe fn add_connection_to_epoll(epfd: RawFd, arc_connection: &Arc<Connection>) {
    let fd = (*arc_connection).fd;
    let result = libc::epoll_ctl(epfd,
                       libc::EPOLL_CTL_ADD,
                       fd,
                       &mut libc::epoll_event { events: EVENTS_R as u32, u64: fd as u64 });

   if result < 0 {
       let err = Error::from_raw_os_error(errno().0 as i32);
       error!("Adding fd to epoll: {}", err);

       // Update state to IoEvent::ShouldClose
       let mut guard = match (*arc_connection).event.lock() {
           Ok(g) => g,
           Err(p) => p.into_inner()
       };
       let mut event_state = guard.deref_mut();
       *event_state = IoEvent::ShouldClose;
   }
}

/// Re-arms a connection in the epoll interest list with the event mask.
unsafe fn rearm_connection_in_epoll(epfd: RawFd, arc_connection: &Arc<Connection>) {
    let mut rw_event = false;
    { // Mutex lock
        let mut guard = match (*arc_connection).state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };
        let io_state = guard.deref_mut();
        if *io_state == IoState::ReArmRW {
            rw_event = true;
        }
    } // Mutex unlock

    let fd = (*arc_connection).fd;
    let events = if rw_event {
        EVENTS_RW as u32
    } else {
        EVENTS_R as u32
    };
    let result = libc::epoll_ctl(epfd,
                       libc::EPOLL_CTL_MOD,
                       fd,
                       &mut libc::epoll_event { events: events, u64: fd as u64 });

    if result < 0 {
        let err = Error::from_raw_os_error(errno().0 as i32);
        error!("Re-arming fd in epoll: {}", err);

        // Update state to IoEvent::ShouldClose
        let mut guard = match (*arc_connection).event.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };
        let mut event_state = guard.deref_mut();
        *event_state = IoEvent::ShouldClose;
    }
}

/// Traverses the ConnectionSlab and updates any connection's state reported changed by epoll.
unsafe fn update_io_events(connection_slab: &ConnectionSlab,
                           arc_io_queue: &IoQueue,
                           events: &[libc::epoll_event])
{
    const READ_EVENT: u32 = libc::EPOLLIN as u32;
    const WRITE_EVENT: u32 = libc::EPOLLOUT as u32;

    for event in events.iter() {
        // Locate the connection this event is for
        let fd = event.u64 as RawFd;
        let find_result = find_connection_from_fd(fd, connection_slab);
        if find_result.is_err() {
            error!("Finding fd: {} in connection_slab", fd);
            continue;
        }

        let arc_connection = find_result.unwrap();

        // Event type branch
        let read_available = (event.events & READ_EVENT) > 0;
        let write_available = (event.events & WRITE_EVENT) > 0;
        if !read_available && !write_available {
            { // Mutex lock
                let mut guard = match arc_connection.err_mutex.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner()
                };
                let err_opt = guard.deref_mut();
                *err_opt = Some(Error::new(ErrorKind::ConnectionAborted, "ConnectionAborted"));
            } // Mutex unlock
            continue;
        }

        let io_event = if read_available && write_available {
            IoEvent::ReadWriteAvailable
        } else if read_available {
            IoEvent::ReadAvailable
        } else {
            IoEvent::WriteAvailable
        };

        let io_pair = IoPair {
            event: io_event,
            arc_connection: arc_connection
        };
        { // Mutex lock
            let mut guard = match arc_io_queue.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };

            let mut io_queue = guard.deref_mut();
            (*io_queue).push(io_pair);
        } // Mutex unlock
    }
}

/// Given a fd and ConnectionSlab, returns the Connection associated with fd.
unsafe fn find_connection_from_fd(fd: RawFd,
                                  connection_slab: &ConnectionSlab)
                                  -> Result<Arc<Connection>, ()>
{
    let slab_ptr = (*connection_slab).inner.get();
    for ref arc_connection in (*slab_ptr).iter() {
        if (*arc_connection).fd == fd {
            return Ok((*arc_connection).clone());
        }
    }

    Err(())
}

unsafe fn io_sentinel(connection_slab: ConnectionSlab,
                      arc_io_queue: IoQueue,
                      thread_pool: ThreadPool,
                      handler: EventHandler)
{
    // We want to wake up with the same interval consitency as the epoll_wait loop.
    // Plus a few ms for hopeful non-interference from mutex contention.
    let _100ms = 1000000 * 100;
    let wait_interval = Duration::new(0, _100ms);

    loop {
        thread::sleep(wait_interval);

        let mut io_queue;
        { // Mutex lock
            let mut guard = match arc_io_queue.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };

            let mut queue = guard.deref_mut();
            let mut empty_queue = Vec::<IoPair>::with_capacity(MAX_EVENTS as usize);
            io_queue = mem::replace(&mut (*queue), empty_queue);
        } // Mutex unlock

        // TODO - Finish this function up
    }
}

unsafe fn handle_socket_writable(arc_connection: Arc<Connection>) {
    let _ = match arc_connection.tx_mutex.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };

    // Get a pointer into UnsafeCell<Stream>
    let stream_ptr = arc_connection.stream.get();

    let empty = Vec::<u8>::new();
    let write_result = (*stream_ptr).send(&empty[..]);
    if write_result.is_ok() {
        let starting_event_state;
        { // Mutex lock
            // This event will always be hit first when epoll has notified us that
            // the fd is both writable and has data available for reading. If this
            // is the case, we need to carefully adjust the state so that when
            // re-arming the fd, we choose the correct event mask.
            let mut event_guard = match arc_connection.event.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };

            let event_state = event_guard.deref_mut();
            starting_event_state = (*event_state).clone();

            if *event_state == IoEvent::DataAvailableAndWritable {
                *event_state = IoEvent::DataAvailable;
            } else {
                *event_state = IoEvent::Waiting;
            }
        } // Mutex unlock

        // We rely on the handle_data_available function to re-arm the fd for reading,
        // so if this fd's event mask did not indicate it was both writable and ready
        // for reading, we need to re-arm it here.
        if starting_event_state == IoEvent::Writable {
            { // Mutex lock
                let mut guard = match arc_connection.state.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner()
                };

                let io_state = guard.deref_mut();
                *io_state = IoState::ReArmR;
            } // Mutex unlock
        }
        return;
    }

    let err = write_result.unwrap_err();
    if err.kind() == ErrorKind::WouldBlock {
        { // Mutex lock
            let mut guard = match arc_connection.state.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };

            // We do not care about other states here, because this encompasses all
            // other options that epoll will update us with.
            let io_state = guard.deref_mut();
            *io_state = IoState::ReArmRW;
        } // Mutex unlock
        return;
    }

    { // Mutex lock
        // If we're in a state of ShouldClose, no need to worry
        // about any other operations...
        let mut event_guard = match arc_connection.event.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };

        let event_state = event_guard.deref_mut();
        if *event_state != IoEvent::ShouldClose {
            *event_state = IoEvent::ShouldClose;
        }
    } // Mutex unlock
}

unsafe fn handle_data_available(arc_connection: Arc<Connection>, handler: EventHandler) {
    // Get a pointer into UnsafeCell<Stream>
    let stream_ptr = (*arc_connection).stream.get();

    // Attempt recv
    match (*stream_ptr).recv() {
        Ok(mut queue) => {
            { // Mutex lock
                let mut event_guard = match (*arc_connection).event.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner()
                };

                let event_state = event_guard.deref_mut();
                *event_state = IoEvent::Waiting;
            } // Mutex unlock

            { // Mutex lock
                // Update the state so that the next iteration over the ConnectionSlab
                // will re-arm this connection in epoll
                let mut guard = match (*arc_connection).state.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner()
                };

                let io_state = guard.deref_mut();
                if *io_state != IoState::ReArmRW {
                    *io_state = IoState::ReArmR;
                }
            } // Mutex unlock

            // Hand off the messages on to the consumer
            for msg in queue.drain(..) {
                let EventHandler(ptr) = handler;
                let hydrogen_socket = HydrogenSocket::new(arc_connection.clone());
                (*ptr).on_data_received(hydrogen_socket, msg);
            }
        }

        Err(err) => {
            let kind = err.kind();
            if kind == ErrorKind::WouldBlock {
                { // Mutex lock
                    let mut event_guard = match (*arc_connection).event.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner()
                    };

                    let event_state = event_guard.deref_mut();
                    *event_state = IoEvent::Waiting;
                } // Mutex unlock

                { // Mutex lock
                    // Update the state so that the next iteration over the ConnectionSlab
                    // will re-arm this connection in epoll
                    let mut guard = match (*arc_connection).state.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner()
                    };

                    let io_state = guard.deref_mut();
                    if *io_state != IoState::ReArmRW {
                        *io_state = IoState::ReArmR;
                    }
                } // Mutex unlock
                return;
            }

            if kind != ErrorKind::UnexpectedEof
                && kind != ErrorKind::ConnectionReset
                && kind != ErrorKind::ConnectionAborted
            {
                error!("During recv: {}", err);
            }

            // Update the state so that the next iteration over the ConnectionSlab
            // will remove this connection.
            let mut guard = match (*arc_connection).event.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };

            let event_state = guard.deref_mut();
            *event_state = IoEvent::ShouldClose;
        }
    };
}
