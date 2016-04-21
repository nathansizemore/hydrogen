// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::io::Error;
use std::cell::UnsafeCell;
use std::sync::{Arc, Mutex};
use std::os::unix::io::{RawFd, AsRawFd};

use simple_slab::Slab;

use super::{Stream, Handler};


/// Memory region for all concurrent connections.
pub type ConnectionSlab = Arc<MutSlab>;
/// Protected memory region for newly accepted connections.
pub type NewConnectionSlab = Arc<Mutex<Slab<Connection>>>;
/// Queue of Connections needing various I/O operations.
pub type IoQueue = Arc<Mutex<Vec<IoPair>>>;

#[derive(Clone, PartialEq, Eq)]
pub enum IoEvent {
    /// Epoll reported data is available on the socket for reading
    ReadAvailable,
    /// Epoll reported the socket is writable
    WriteAvailable,
    /// Epoll reported the socket is both writable and has data available for reading
    ReadWriteAvailable
}

#[derive(Clone)]
pub struct IoPair {
    /// The type of I/O needed on this Connection
    pub event: IoEvent,
    /// The connection `event` is paired with
    pub arc_connection: Arc<Connection>
}

pub struct Connection {
    /// Underlying file descriptor.
    pub fd: RawFd,
    /// A Some(Error) options means this connection is in
    /// an error'd state and should be closed.
    pub err_mutex: Mutex<Option<Error>>,
    /// Mutex to ensure thread safe, ordered writes to our streams.
    /// They may have internal buffers
    pub tx_mutex: Mutex<()>,
    /// Socket (Stream implemented trait-object).
    pub stream: Arc<UnsafeCell<Stream>>
}
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}


pub struct MutSlab {
    pub inner: UnsafeCell<Slab<Arc<Connection>>>
}
unsafe impl Send for MutSlab {}
unsafe impl Sync for MutSlab {}

pub struct EventHandler(pub *mut Handler);
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

pub struct HydrogenSocket {
    /// The connection this socket represents
    arc_connection: Arc<Connection>
}

impl HydrogenSocket {
    pub fn new(arc_connection: Arc<Connection>) -> HydrogenSocket {
        HydrogenSocket {
            arc_connection: arc_connection
        }
    }

    pub fn send(&self, buf: &[u8]) {
        let _ = match self.arc_connection.tx_mutex.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        };

        let stream_ptr = self.arc_connection.stream.get();
        let write_result = unsafe {
            (*stream_ptr).send(buf)
        };
        if write_result.is_ok() {
            return;
        }

        // let err = write_result.unwrap_err();
        // if err.kind() == ErrorKind::WouldBlock {
        //     { // Mutex lock
        //         let mut guard = match self.arc_connection.state.lock() {
        //             Ok(g) => g,
        //             Err(p) => p.into_inner()
        //         };
        //
        //         // We do not care about other states here, because this encompasses all
        //         // other options that epoll will update us with.
        //         let io_state = guard.deref_mut();
        //         *io_state = IoState::ReArmRW;
        //     } // Mutex unlock
        //     return;
        // }
        //
        // { // Mutex lock
        //     // If we're in a state of ShouldClose, no need to worry
        //     // about any other operations...
        //     let mut event_guard = match self.arc_connection.event.lock() {
        //         Ok(g) => g,
        //         Err(p) => p.into_inner()
        //     };
        //
        //     let event_state = event_guard.deref_mut();
        //     if *event_state != IoEvent::ShouldClose {
        //         *event_state = IoEvent::ShouldClose;
        //     }
        // } // Mutex unlock
    }
}

impl AsRawFd for HydrogenSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.arc_connection.fd
    }
}
