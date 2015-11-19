// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.






pub struct Socket {
    fd: c_int
}

pub enum SocketError {
    /// Passed buffer had a size less than 1
    InvalidBuffer
}

impl Read for Socket {
    extern "C" {
        fn read(fd: c_int, buffer: *mut c_void, count: size_t) -> ssize_t;
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.len() < 1 {
            return Err(InvalidBuffer)
        }

        let num_read = unsafe { read(self.fd, buf as *mut c_void, buf.len()) };
        if num_read < 0 {
            let errno = errno().0 as i32;
            return match errno {
                libc::ENOMEM         => Err(ReadError::ENOMEM),
                libc::EBADF          => Err(ReadError::EBADF),
                libc::EFAULT         => Err(ReadError::EFAULT),
                libc::EINTR          => Err(ReadError::EINTR),
                libc::EINVAL         => Err(ReadError::EINVAL),
                libc::EIO            => Err(ReadError::EIO),
                libc::EISDIR         => Err(ReadError::EISDIR),
                libc::ECONNRESET     => Err(ReadError::ECONNRESET),

                // These two constants differ between OSes, so we can't use a match clause
                x if x == libc::EAGAIN || x == libc::EWOULDBLOCK => Err(ReadError::EAGAIN),

                _ => panic!("Unexpected errno during read: {}", errno)
            };
        }
    }
}
