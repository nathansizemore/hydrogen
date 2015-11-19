// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


extern crate libc;
extern crate errno;


use std::io::{Read, Write, Error, ErrorKind};
use self::libc::{c_int, c_void, size_t, ssize_t};


extern "C" {
    fn read(fd: c_int, buffer: *mut c_void, count: size_t) -> ssize_t;
    fn write(fd: c_int, buffer: *const c_void, cout: size_t) -> ssize_t;
}

pub struct Socket {
    fd: c_int
}


#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ReadError {
    /// System did not have enough memory to complete the read operation.
    NoMemory,
    /// The file descriptor fd refers to a file other than a socket and has been
    /// marked nonblocking (O_NONBLOCK), and the read would block.
    WouldBlock,
    /// This could be several things:
    ///
    /// * Fd is not a valid file descriptor.
    /// * Fd is not open for reading.
    /// * Fd is attached to an object which is unsuitable for reading
    /// * The file was opened with the `O_DIRECT` flag, and either the address specified in buf,
    ///   the value specified in count, or the current file offset is not suitably aligned.
    /// * fd was created via a call to timerfd_create(2) and the wrong size buffer was given to
    ///   `read()`
    InvalidFd,
    /// buf is outside your accessible address space.
    MemAccess,
    /// The call was interrupted by a signal before any data was read.
    /// [see signal(7)][signal7]
    /// [signal7]: http://man7.org/linux/man-pages/man7/signal.7.html
    Signal,
    /// I/O error. This will happen for example when the process is in a background process
    /// group, tries to read from its controlling terminal, and either it is ignoring or
    /// blocking SIGTTIN or its process group is orphaned. It may also occur when there
    /// is a low-level I/O error while reading from a disk or tape.
    Io,
    /// End of file has been reached or fd is connected to a TCP socket where the other end has
    /// been closed, but the write command has not had enough time to finish writing to the
    /// internal buffer. No more can be read from this socket.
    Closed
}

impl ReadError {
    pub fn from_errno(errno: i32) -> ReadError {
        match errno {
            libc::ENOMEM => ReadError::NoMemory,
            libc::EBADF => ReadError::InvalidFd,
            libc::EFAULT => ReadError::MemAccess,
            libc::EINTR => ReadError::Signal,
            libc::EINVAL => ReadError::InvalidFd,
            libc::EIO => ReadError::Io,
            libc::EISDIR => ReadError::InvalidFd,
            libc::ECONNRESET => ReadError::Closed,
            libc::EOF => ReadError::Closed,
            libc::EWOULDBLOCK => ReadError::WouldBlock,
            libc::EAGAIN => ReadError::WouldBlock,

            _ => panic!("Unexpected errno during read: {}", errno)
        }
    }
}

impl

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ReadError> {
        if buf.len() < 1 {
            return Err(ReadError::InvalidBuffer)
        }

        let result = unsafe { read(self.fd, buf as *mut c_void, buf.len()) };
        if result < 0 {
            Err(ReadError::from_errno(result))
        }

        Ok(result as usize)
    }
}
