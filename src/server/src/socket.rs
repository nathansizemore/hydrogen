// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.
//
// This Source Code Form is "Incompatible
// With Secondary Licenses", as defined by
// the Mozilla Public License, v. 2.0.


//! Socket module


use super::rand;
use super::simple_stream::{
    SimpleStream,
    ReadResult,
    WriteResult
};


/// Represents an epoll controlled Async socket
#[derive(Clone)]
pub struct Socket {
    /// Unique identifier
    id: u32,
    /// I/O stream
    stream: SimpleStream
}


impl Socket {

    /// Returns a new Socket
    pub fn new(stream: SimpleStream) -> Socket {
        Socket {
            id: rand::random::<u32>(),
            stream: stream
        }
    }

    /// Returns this socket's id
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Attempts to read from socket
    pub fn read(&mut self) -> ReadResult {
        self.stream.read()
    }

    /// Attempts to write to this socket
    pub fn write(&mut self, buf: &Vec<u8>) -> WriteResult {
        self.write(buf)
    }
}
