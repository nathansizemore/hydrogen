// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


extern crate libc;
extern crate errno;

use std::os::unix::io::{RawFd, AsRawFd};
use std::io::{Read, Write, Error, ErrorKind};

use self::libc::{c_int, c_void};

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
            return Err(Error::from_raw_os_error(result as i32))
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
            return Err(Error::from_raw_os_error(result as i32))
        }

        Ok(result as usize)
    }

    fn flush(&mut self) -> Result<(), Error> { Ok(()) }
}

impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> RawFd { self.fd }
}


// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum ReadError {
//     /// System did not have enough memory to complete the read operation.
//     NoMemory,
//     /// The file descriptor fd refers to a file other than a socket and has been
//     /// marked nonblocking (O_NONBLOCK), and the read would block.
//     WouldBlock,
//     /// This could be several things:
//     ///
//     /// * Fd is not a valid file descriptor.
//     /// * Fd is not open for reading.
//     /// * Fd is attached to an object which is unsuitable for reading
//     /// * The file was opened with the `O_DIRECT` flag, and either the address specified in buf,
//     ///   the value specified in count, or the current file offset is not suitably aligned.
//     /// * fd was created via a call to timerfd_create(2) and the wrong size buffer was given to
//     ///   `read()`
//     InvalidFd,
//     /// buf is outside your accessible address space.
//     MemAccess,
//     /// The call was interrupted by a signal before any data was read.
//     /// [see signal(7)][signal7]
//     /// [signal7]: http://man7.org/linux/man-pages/man7/signal.7.html
//     Signal,
//     /// I/O error. This will happen for example when the process is in a background process
//     /// group, tries to read from its controlling terminal, and either it is ignoring or
//     /// blocking SIGTTIN or its process group is orphaned. It may also occur when there
//     /// is a low-level I/O error while reading from a disk or tape.
//     Io,
//     /// End of file has been reached or fd is connected to a TCP socket where the other end has
//     /// been closed, but the write command has not had enough time to finish writing to the
//     /// internal buffer. No more can be read from this socket.
//     Closed,
//     /// Passed buffer length was less than 1
//     InvalidBuffer
// }
//
// impl ReadError {
//     pub fn from_errno(errno: i32) -> ReadError {
//         match errno {
//             libc::ENOMEM => ReadError::NoMemory,
//             libc::EBADF => ReadError::InvalidFd,
//             libc::EFAULT => ReadError::MemAccess,
//             libc::EINTR => ReadError::Signal,
//             libc::EINVAL => ReadError::InvalidFd,
//             libc::EIO => ReadError::Io,
//             libc::EISDIR => ReadError::InvalidFd,
//             libc::ECONNRESET => ReadError::Closed,
//             libc::EOF => ReadError::Closed,
//             libc::EWOULDBLOCK => ReadError::WouldBlock,
//
//             _ => panic!("Unexpected errno during read: {}", errno)
//         }
//     }
// }
//
// impl fmt::Display for ReadError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match *self {
//             ReadError::NoMemory => "NoMemory".fmt(f),
//             ReadError::WouldBlock => "WouldBlock".fmt(f),
//             ReadError::InvalidFd => "InvalidFd".fmt(f),
//             ReadError::MemAccess => "MemAccess".fmt(f),
//             ReadError::Signal => "Signal".fmt(f),
//             ReadError::Io => "Io".fmt(f),
//             ReadError::Closed => "Closed".fmt(f),
//             ReadError::InvalidBuffer => "InvalidBuffer".fmt(f)
//         }
//     }
// }
//
// impl error::Error for ReadError {
//     fn description(&self) -> &str {
//         match *self {
//             ReadError::NoMemory => "NoMemory",
//             ReadError::WouldBlock => "WouldBlock",
//             ReadError::InvalidFd => "InvalidFd",
//             ReadError::MemAccess => "MemAccess",
//             ReadError::Signal => "Signal",
//             ReadError::Io => "Io",
//             ReadError::Closed => "Closed",
//             ReadError::InvalidBuffer => "InvalidBuffer"
//         }
//     }
// }
//
//
//
// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum WriteError {
//     /// The file descriptor fd refers to a file other than a socket and has been
//     /// marked nonblocking (O_NONBLOCK), and the read would block.
//     WouldBlock,
//     /// This could be several things:
//     ///
//     /// * Fd is not a valid file descriptor.
//     /// * Fd is not open for writing.
//     /// * Fd is attached to an object which is unsuitable for writing
//     /// * The file was opened with the `O_DIRECT` flag, and either the address specified in buf,
//     ///   the value specified in count, or the current file offset is not suitably aligned.
//     InvalidFd,
//     /// buf is outside your accessible address space.
//     MemAccess,
//     /// The call was interrupted by a signal before any data was read.
//     /// [see signal(7)][signal7]
//     /// [signal7]: http://man7.org/linux/man-pages/man7/signal.7.html
//     Signal,
//     /// A low-level I/O error occurred while modifying the inode.
//     Io,
//     /// fd is connected to a pipe or socket whose reading end is closed. When this happens
//     /// the writing process will also receive a SIGPIPE signal. (Thus, the write return value
//     /// is seen only if the program catches, blocks or ignores this signal.) Or, fd is connected
//     /// to a TCP socket where the other end has been closed, but the write command has not had
//     /// enough time to finish writing to the internal buffer.
//     Closed
// }
//
// impl WriteError {
//     pub fn from_errno(errno: i32) -> WriteError {
//         match errno {
//             libc::EAGAIN => WriteError::WouldBlock,
//             libc::EBADF => WriteError::InvalidFd,
//             libc::EFAULT => WriteError::MemAccess,
//             libc::EINTR => WriteError::Signal,
//             libc::EINVAL => WriteError::InvalidFd,
//             libc::EIO => WriteError::Io,
//             libc::EPIPE => WriteError::Closed,
//             libc::ECONNRESET => WriteError::Closed,
//
//             _ => panic!("Unexpected errno during read: {}", errno)
//         }
//     }
// }
//
// impl fmt::Display for WriteError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         match *self {
//             WriteError::WouldBlock => "WouldBlock".fmt(f),
//             WriteError::InvalidFd => "InvalidFd".fmt(f),
//             WriteError::MemAccess => "MemAccess".fmt(f),
//             WriteError::Signal => "Signal".fmt(f),
//             WriteError::Io => "Io".fmt(f),
//             WriteError::Closed => "Closed".fmt(f)
//         }
//     }
// }
//
// impl error::Error for WriteError {
//     fn description(&self) -> &str {
//         match *self {
//             WriteError::WouldBlock => "WouldBlock",
//             WriteError::InvalidFd => "InvalidFd",
//             WriteError::MemAccess => "MemAccess",
//             WriteError::Signal => "Signal",
//             WriteError::Io => "Io",
//             WriteError::Closed => "Closed"
//         }
//     }
// }
