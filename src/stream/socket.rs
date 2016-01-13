// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::os::unix::io::{RawFd, AsRawFd};
use std::io::{Read, Write, Error, ErrorKind};

use libc;
use errno::errno;
use libc::{c_int, c_void};

#[derive(Clone)]
pub struct Socket {
    /// This socket's file descriptor
    pub fd: c_int
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if buf.len() < 1 {
            return Err(Error::new(ErrorKind::Other, "Invalid buffer"))
        }

        let result = unsafe {
            libc::read(self.fd, buf as *mut _ as *mut c_void, buf.len())
        };

        if result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32))
        }

        if result == 0 {
            return Err(Error::new(ErrorKind::Other, "EOF"));
        }

        Ok(result as usize)
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        let result = unsafe {
            libc::write(self.fd, buf as *const _ as *const c_void, buf.len())
        };

        if result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32))
        }

        Ok(result as usize)
    }

    fn flush(&mut self) -> Result<(), Error> { Ok(()) }
}

impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> RawFd { self.fd }
}
