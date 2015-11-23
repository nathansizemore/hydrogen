// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


//! Various types used throughout the server crate


use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::collections::LinkedList;
use std::os::unix::io::AsRawFd;

use stream::nbstream::Nbstream;

pub trait EventHandler {
    fn on_data_received(&mut self, stream: Nbstream, buffer: Vec<u8>);
    fn on_stream_closed(&mut self, id: String);
}

/// Thread safe LinkedList<T: Stream>
pub type StreamList = Arc<Mutex<LinkedList<Nbstream>>>;

/// Thread safe EventHandler
pub type SafeHandler = Arc<Mutex<EventHandler + Send + Sync + 'static>>;

/// FnOnce signature for EventHandler on_data_received fn
pub type EventHandlerFn = Arc<Mutex<(Fn() + Send + Sync + 'static)>>;
