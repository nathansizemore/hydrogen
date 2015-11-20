// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


// TODO - Implement error handling for all spots where an OS error can occur

use std::os::unix::io::AsRawFd;
use std::io::{Read, Write, Error, ErrorKind};

use socket::Socket;



pub struct Bstream<T: Read + Write + AsRawFd> {
    inner: T,
    scratch: Vec<u8>
}

impl Bstream {
    pub fn new<T: Read + Write + AsRawFd>(stream: T) -> Bstream<T> {
        let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL, 0) };
        let bflags = flags & (~libc::O_NONBLOCK);
        let result = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL, bflags) };
        Bstream { inner: stream }
    }

    pub fn recv(&mut self) -> Result<Vec<u8>, Error> {
        let p_len;
        let mut state = FrameState::Start;
        let mut p_len_buf = Vec::<u8>::with_capacity(2);
        let mut payload = Vec::<u8>::with_capacity(512);

        loop {
            // Read in chunks until we find a start byte
            let mut tbuf = [0u8 ..512];
            let result = self.inner.read(&mut tbuf);
            if result.is_err() {
                return result.unwrap_err()
            }
            let num_read = result.unwrap();

            // We might be in the middle of separate tcp frames for a complete message,
            // so we need to dump our just read buffer after our current scratch section
            let mut buf;
            if self.scratch.len() > 0 {
                buf = Vec::<u8>::with_capacity(num_read + self.scratch.len());
                for byte in self.scratch.iter() {
                    buf.push(byte);
                }
                self.scratch = Vec::<u8>::new();
                for x in 0..num_read {
                    buf.push(tbuf[x]);
                }
            } else {
                buf = tbuf;
            }

            let mut seek_pos = 0usize;
            if state == FrameState::Start {
                for _ in 0..num_read {
                    if self.scratch[seek_pos] == frame::START {
                        state = FrameState::PayloadLen;
                        seek_pos += 1;
                        break;
                    }
                    seek_pos += 1;
                }
            }

            if state == FrameState::PayloadLen {
                for _ in seek_pos..num_read {
                    p_len.push(buf[seek_pos]);
                    if p_len.len() == 2 {
                        let clear_mask = 0xFFFFu16;
                        let mut len = (p_len[0] as u16) & clear_mask;
                        p_len = len | p_len[1] as u16;

                        state = FrameState::Payload;
                        seek_pos += 1;
                        break;
                    }
                    seek_pos += 1;
                }
            }

            if state == FrameState::Payload {
                for _ in seek_pos..num_read {
                    payload.push(buf[seek_pos]);
                    if payload.len() == len {
                        state = FrameState::End;
                        seek_pos += 1;
                        break;
                    }
                    seek_pos += 1;
                }
            }

            if state == FrameState::End {
                for _ in seek_pos..num_read {
                    if buf[seek_pos] == frame::END {
                        if payload.len() == p_len {
                            payload.truncate(p_len);
                            state = FrameState::Scratch;
                            seek_pos += 1;
                            break;
                        }
                    }
                    seek_pos +=1;
                }
            }

            if state == FrameState::Scratch {
                let scratch_size = num_read - seek_pos;
                self.scratch = Vec::<u8>::with_capacity(scratch_size);
                for _ in seek_pos..num_read {
                    self.scratch.push(buf[seek_pos]);
                    seek_pos += 1;
                }
                return Ok(payload)
            }
        }
    }
}
