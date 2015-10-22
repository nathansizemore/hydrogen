// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


//! Socket module
#![allow(dead_code)]


use std::os::unix::io::RawFd;

use stats;
use super::rand;
use super::simple_stream::nbetstream::{
    NbetStream,
    ReadResult,
    WriteResult
};


/// Represents an epoll controlled Async socket
#[derive(Clone)]
pub struct Socket {
    /// Unique identifier
    id: u32,
    /// I/O stream
    stream: NbetStream
}


impl Socket {

    /// Returns a new Socket
    pub fn new(stream: NbetStream) -> Socket {
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
        let result = self.stream.write(buf);
        if result.is_ok() {
            stats::msg_sent();
            stats::bytes_sent(buf.len());
        }
        result
    }

    /// Returns the underlying file descriptor
    pub fn raw_fd(&self) -> RawFd {
        self.stream.raw_fd()
    }

    /// Returns a Vec<Vec<u8>> representing the payloads that were read during the last
    /// call to read().
    /// This calls drains the internal buffer
    pub fn buffer(&mut self) -> Vec<Vec<u8>> {
        let internal_buffer = self.stream.buffer_as_mut();
        let queue = internal_buffer.drain_queue();
        queue
    }
}
