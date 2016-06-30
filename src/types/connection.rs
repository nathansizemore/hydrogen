// Copyright 2016 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.


use std::{io, marker};
use std::sync::Mutex;
use std::cell::UnsafeCell;
use std::os::unix::io::{AsRawFd, RawFd};


use ::Stream;


pub struct Connection<T: Stream> {
    inner: UnsafeCell<T>,
    rx_mutex: Mutex<()>,
    tx_mutex: Mutex<()>
}

impl<T: Stream> Connection<T> {
    pub fn new(stream: T) -> Connection<T> {
        Connection {
            inner: UnsafeCell::new(stream),
            rx_mutex: Mutex::new(()),
            tx_mutex: Mutex::new(())
        }
    }

    pub fn recv(&self) -> io::Result<Vec<Vec<u8>>> {
        let _ = self.rx_mutex.lock().unwrap();
        let stream_ptr = self.inner.get();
        unsafe { (*stream_ptr).recv() }
    }

    pub fn send(&self, buf: &[u8]) -> io::Result<()> {
        let _ = self.tx_mutex.lock().unwrap();
        let stream_ptr = self.inner.get();
        unsafe { (*stream_ptr).send(buf) }
    }

    pub fn shutdown(&self) {
        let stream_ptr = self.inner.get();
        unsafe { (*stream_ptr).shutdown() }
    }
}

impl<T: Stream> AsRawFd for Connection<T> {
    fn as_raw_fd(&self) -> RawFd {
        let stream_ptr = self.inner.get();
        unsafe { (*stream_ptr).as_raw_fd() }
    }
}

unsafe impl<T: Stream> Sync for Connection<T> {}
unsafe impl<T: Stream> marker::Send for Connection<T> {}
