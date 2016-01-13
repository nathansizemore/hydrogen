// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::os::unix::io::{RawFd, AsRawFd};
use std::io::Error;


pub mod frame;
pub mod socket;
pub mod nbstream;


pub trait HRecv {
    fn recv(&mut self) -> Result<(), Error>;
    fn drain_rx_queue(&mut self) -> Vec<Vec<u8>>;
}
pub trait HSend {
    fn send(&mut self, buf: &[u8]) -> Result<usize, Error>;
}
pub trait CloneHStream { fn clone_h_stream(&self) -> Box<HStream>; }
pub trait HStream: HRecv + HSend + CloneHStream + AsRawFd {}


pub struct Stream {
    pub inner: Box<HStream>
}

impl Clone for Stream {
    fn clone(&self) -> Stream {
        Stream {
            inner: self.inner.clone_h_stream()
        }
    }
}

impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl<T> CloneHStream for T where T: 'static + Clone + HStream {
    fn clone_h_stream(&self) -> Box<HStream> {
        Box::new(self.clone())
    }
}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}


impl HRecv for Stream {
    fn recv(&mut self) -> Result<(), Error> {
        self.inner.recv()
    }

    fn drain_rx_queue(&mut self) -> Vec<Vec<u8>> {
        self.inner.drain_rx_queue()
    }
}

impl HSend for Stream {
    fn send(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.inner.send(buf)
    }
}
