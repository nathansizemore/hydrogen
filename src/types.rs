// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::sync::{Arc, Mutex};
use std::collections::LinkedList;
use std::os::unix::io::RawFd;

use ss::Stream;

/// The `EventHandler` trait allows for hydrogen event dispatching.
///
/// Hydrogen uses multiple threads to dispatch events. Implementors should
/// take care to ensure any state within `self` is properly safeguarded
/// against race conditions that may occur.
pub trait EventHandler {
    fn on_data_received(&mut self, stream: Stream, buffer: Vec<u8>);
    fn on_stream_closed(&mut self, fd: RawFd);
}

/// Internal list of all currently connected streams
pub type StreamList = Arc<Mutex<LinkedList<Stream>>>;

/// Used as a strongly typed wrapper for passing around `EventHandler`
pub struct Handler(pub *mut EventHandler);
unsafe impl Send for Handler {}
unsafe impl Sync for Handler {}
impl Clone for Handler {
    fn clone(&self) -> Handler {
        let Handler(ptr) = *self;
        unsafe {
            let same_location = &mut *ptr;
            Handler(same_location)
        }
    }
}

/// Used as a strongly typed wrapper for passing around `EventHandler` functions
pub struct Event(pub *mut Fn());
unsafe impl Send for Event {}
unsafe impl Sync for Event {}
impl Clone for Event {
    fn clone(&self) -> Event {
        let Event(ptr) = *self;
        unsafe {
            let same_location = &mut *ptr;
            Event(same_location)
        }
    }
}
