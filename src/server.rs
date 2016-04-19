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

use config::Config;
use super::{Stream, Handler};


// When added to epoll, these will be the conditions of kernel notification:
//
// EPOLLIN          - Data is available in kerndl buffer.
// EPOLLRDHUP       - Peer closed connection.
// EPOLLPRI         - Urgent data for read available.
// EPOLLET          - Register in EdgeTrigger mode.
// EPOLLONESHOT     - After an event is pulled out with epoll_wait(2) the associated
//                    file descriptor is internally disabled and no other events will
//                    be reported by the epoll interface.
const EVENTS: i32 = libc::EPOLLIN |
                    libc::EPOLLRDHUP |
                    libc::EPOLLPRI |
                    libc::EPOLLET |
                    libc::EPOLLONESHOT;


#[derive(Clone, PartialEq, Eq)]
enum IoEvent {
    /// Waiting for an updte from epoll
    Waiting,
    /// Epoll has reported data is waiting to be read from socket.
    DataAvailable,
    /// Error/Disconnect/Etc has occured and socket needs removed from server.
    ShouldClose
}

#[derive(Clone, PartialEq, Eq)]
enum IoState {
    /// New connection, needs added to epoll instance.
    New,
    /// Socket has no data avialable for reading, but is armed and in the
    /// epoll instance's interest list.
    Waiting,
    /// Socket is currently in use (reading).
    InUse,
    /// All I/O operations have been exhausted and socket is ready to be
    /// re-inserted into the epoll instance's interest list.
    ReArm
}

struct EventHandler(pub *mut Handler);
unsafe impl Send for EventHandler {}
unsafe impl Sync for EventHandler {}
impl Clone for EventHandler {
    fn clone(&self) -> EventHandler {
        let EventHandler(ptr) = *self;
        unsafe {
            let same_location = &mut *ptr;
            EventHandler(same_location)
        }
    }
}

struct Connection {
    /// Underlying file descriptor.
    fd: RawFd,
    /// Last reported event fired from epoll.
    event: Mutex<IoEvent>,
    /// Current I/O state for socket.
    state: Mutex<IoState>,
    /// Socket (Stream implemented trait-object).
    stream: Arc<UnsafeCell<Stream>>
}
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}


struct MutSlab {
    inner: UnsafeCell<Slab<Arc<Connection>>>
}
unsafe impl Send for MutSlab {}
unsafe impl Sync for MutSlab {}


/// Memory region for all concurrent connections.
type ConnectionSlab = Arc<MutSlab>;

/// Protected memory region for newly accepted connections.
type NewConnectionSlab = Arc<Mutex<Slab<Connection>>>;


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
    let stream = (*handler_ptr).on_new_connection(fd);

    // Create a connection structure
    let connection = Connection {
        fd: fd,
        event: Mutex::new(IoEvent::Waiting),
        state: Mutex::new(IoState::New),
        stream: stream
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
    // Maximum number of events returned from epoll_wait
    const MAX_EVENTS: i32 = 100;
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

    // Scratch space for epoll returned events
    let mut event_buffer = Vec::<libc::epoll_event>::with_capacity(MAX_EVENTS as usize);
    event_buffer.set_len(MAX_EVENTS as usize);

    loop {
        // Remove any connections in the IoState::ShouldClose state.
        remove_stale_connections(&connection_slab, &thread_pool, &handler);

        // Insert any newly received connections into the connection_slab
        insert_new_connections(&new_connections, &connection_slab);

        // Adjust states/re-arm connections before going back into epoll_wait
        prepare_connections_for_epoll_wait(epfd, &connection_slab);

        // Check for any new events
        let result = libc::epoll_wait(epfd, event_buffer.as_mut_ptr(), MAX_EVENTS, MAX_WAIT);
        if result < 0 {
            let err = Error::from_raw_os_error(errno().0 as i32);
            error!("During epoll_wait: {}", err);
            panic!();
        }

        let num_events = result as usize;
        update_io_events(&connection_slab, &event_buffer[0..num_events]);
    }
}

/// Traverses through the connection slab and creates a list of connections that need dropped,
/// then traverses that list, drops them, and informs the handler of client drop.
unsafe fn remove_stale_connections(connection_slab: &ConnectionSlab,
                                   thread_pool: &ThreadPool,
                                   handler: &EventHandler)
{
    let slab_ptr = (*connection_slab).inner.get();
    let max_removals = (*slab_ptr).len();
    let mut offsets = Vec::<usize>::with_capacity(max_removals);

    for x in 0..max_removals {
        match (*slab_ptr)[x] {
            Some(ref arc_connection) => {
                // Assuming spin locks would be a solid choice here, but not
                // currently a direct API from stdlib. Possibly could use the
                // `try_lock()` function from Mutex with a timer and loop, but that
                // just seems a bit trickier to get right than what I'm willing to
                // devote during this loop.
                let event_state: IoEvent;
                { // Mutex lock
                    let guard = match (*arc_connection).event.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner()
                    };
                    event_state = (*guard).clone();
                } // Mutex unlock

                if event_state == IoEvent::ShouldClose {
                    offsets.push(x);
                }
            }
            None => { }
        }
    }

    for offset in offsets {
        let arc_connection = (*slab_ptr).remove(offset).unwrap();

        // Inform kernel we're done
        close_connection(&arc_connection);

        // Inform the consumer connection is no longer valid
        let fd = (*arc_connection).fd;
        let handler_clone = (*handler).clone();
        thread_pool.execute(move || {
            let EventHandler(ptr) = handler_clone;
            (*ptr).on_connection_removed(fd);
        });
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
                                 connection_slab: &ConnectionSlab)
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
        (*arc_main_slab).insert(Arc::new(connection));
    }
}

/// Traverses the "main" connection_slab looking for connections in the IoState::New
/// or IoState::ReArm it then either adds the new connection to epoll's interest list,
/// or re-arms the fd with a new event mask.
unsafe fn prepare_connections_for_epoll_wait(epfd: RawFd, connection_slab: &ConnectionSlab) {
    // Unwrap/dereference our Slab from Arc<Unsafe<T>>
    let slab_ptr = (*connection_slab).inner.get();

    // Iterate over our connections and add them to epoll if they are new,
    // or re-arm them epoll if they are finished with I/O operations
    for ref arc_connection in (*slab_ptr).iter_mut() {
        let mut guard = match (*arc_connection).state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };

        let mut io_state = guard.deref_mut();
        if *io_state == IoState::New {
            add_connection_to_epoll(epfd, arc_connection);
            *io_state = IoState::Waiting;
        } else if *io_state == IoState::ReArm {
            rearm_connection_in_epoll(epfd, arc_connection);
            *io_state = IoState::Waiting;
        }
    }
}

/// Adds a new connection to the epoll interest list.
unsafe fn add_connection_to_epoll(epfd: RawFd, arc_connection: &Arc<Connection>) {
    let fd = (*arc_connection).fd;
    let result = libc::epoll_ctl(epfd,
                       libc::EPOLL_CTL_ADD,
                       fd,
                       &mut libc::epoll_event { events: EVENTS as u32, u64: fd as u64 });

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
    let fd = (*arc_connection).fd;
    let result = libc::epoll_ctl(epfd,
                       libc::EPOLL_CTL_MOD,
                       fd,
                       &mut libc::epoll_event { events: EVENTS as u32, u64: fd as u64 });

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
unsafe fn update_io_events(connection_slab: &ConnectionSlab, events: &[libc::epoll_event]) {
    const READ_EVENT: u32 = libc::EPOLLIN as u32;

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
        let data_available = (event.events & READ_EVENT) > 0;

        // If we've properly handled race conditions correctly and state ordering,
        // the only thing we care about here are connections that are currently
        // in the IoEvent::Waiting state.
        let mut guard = match (*arc_connection).event.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };

        let io_event = guard.deref_mut();

        if data_available {
            if *io_event == IoEvent::Waiting {
                *io_event = IoEvent::DataAvailable;
            }
        } else {
            *io_event = IoEvent::ShouldClose;
        }
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
                      thread_pool: ThreadPool,
                      handler: EventHandler)
{
    // We want to wake up with the same interval consitency as the epoll_wait loop.
    // Plus a few ms for hopeful non-interference from mutex contention.
    let _100ms = 1000000 * 100;
    let wait_interval = Duration::new(0, _100ms);

    loop {
        thread::sleep(wait_interval);

        // Go through the connection_slab and process any connections that
        // are in the IoEvent::DataAvailable state.
        let slab_ptr = (*connection_slab).inner.get();
        for ref arc_connection in (*slab_ptr).iter() {
            let mut event_guard = match (*arc_connection).event.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner()
            };

            let event_state = event_guard.deref_mut();
            if *event_state == IoEvent::DataAvailable {
                let mut state_guard = match (*arc_connection).state.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner()
                };

                let io_state = state_guard.deref_mut();
                if *io_state == IoState::Waiting {
                    // Update state so other threads do not interfere
                    // with this connection.
                    *io_state = IoState::InUse;

                    let conn_clone = (*arc_connection).clone();
                    let handler_clone = handler.clone();
                    thread_pool.execute(move || {
                        handle_data_available(conn_clone, handler_clone);
                    });
                }
            }
        }
    }
}

unsafe fn handle_data_available(arc_connection: Arc<Connection>, handler: EventHandler) {
    // Get a pointer into UnsafeCell<Stream>
    let stream_ptr = (*arc_connection).stream.get();

    // Attempt recv
    let recv_result = (*stream_ptr).recv();
    if recv_result.is_err() {
        let err = recv_result.unwrap_err();
        let kind = err.kind();

        trace!("Error during recv: {}", err);

        if kind != ErrorKind::UnexpectedEof
            && kind != ErrorKind::ConnectionReset
            && kind != ErrorKind::ConnectionAborted
            && kind != ErrorKind::WouldBlock
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
        return;
    }

    // Update the state so that the next iteration over the ConnectionSlab
    // will re-arm this connection in epoll
    let mut guard = match (*arc_connection).state.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner()
    };

    let io_state = guard.deref_mut();
    *io_state = IoState::ReArm;

    // Hand off the messages on to the consumer
    let mut queue = recv_result.unwrap();
    for msg in queue.drain(..) {
        let EventHandler(ptr) = handler;
        let arc_stream = (*arc_connection).stream.clone();
        (*ptr).on_data_received(arc_stream, msg);
    }
}
