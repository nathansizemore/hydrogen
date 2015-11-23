// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::os::unix::io::RawFd;
use std::io::{Error, ErrorKind};

use libc;
use frame;
use rand;
use rand::Rng;
use errno::errno;
use socket::Socket;
use frame::FrameState;

#[derive(Clone)]
pub struct Nbstream {
    id: String,
    inner: Socket,
    state: FrameState,
    buffer: Vec<u8>,
    scratch: Vec<u8>,
    tx_queue: Vec<Vec<u8>>,
    rx_queue: Vec<Vec<u8>>
}

impl Nbstream {
    pub fn new(stream: Socket) -> Result<Nbstream, Error> {
        let result = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
        if result < 0 {
            return Err(Error::from_raw_os_error(errno().0 as i32))
        }

        Ok(Nbstream {
            id: rand::thread_rng().gen_ascii_chars().take(15).collect::<String>(),
            inner: stream,
            state: FrameState::Start,
            buffer: Vec::with_capacity(3),
            scratch: Vec::new(),
            tx_queue: Vec::new(),
            rx_queue: Vec::new()
        })
    }

    pub fn id(&self) -> String {
        self.id.clone()
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }

    pub fn state(&self) -> FrameState {
        self.state.clone()
    }

    pub fn recv(&mut self) -> Result<(), Error> {
        loop {
            let mut buf = Vec::<u8>::with_capacity(512);
            unsafe { buf.set_len(512); }
            let result = self.inner.read(&mut buf[..]);
            if result.is_err() {
                let err = result.unwrap_err();
                if err.kind() == ErrorKind::WouldBlock {
                    println!("Received WouldBlock");
                    return Ok(())
                }
                return Err(err)
            }
            let num_read = result.unwrap();
            println!("read: {}bytes", num_read);

            buf = self.buf_with_scratch(&buf[..], num_read);
            let mut seek_pos = 0usize;

            if self.state == FrameState::Start {
                self.read_for_frame_start(&buf[..], &mut seek_pos, num_read);
            }

            if self.state == FrameState::PayloadLen {
                self.read_payload_len(&buf[..], &mut seek_pos, num_read);
            }

            if self.state == FrameState::Payload {
                self.read_payload(&buf[..], &mut seek_pos, num_read);
            }

            if self.state == FrameState::End {
                let result = self.read_for_frame_end(&buf[..], seek_pos, num_read);
                if result.is_ok() {
                    self.rx_queue.push(result.unwrap());
                }
            }
        }
    }

    pub fn send(&mut self, buf: &[u8]) -> Result<usize, Error> {
        let mut total_written = 0usize;
        self.tx_queue.push(frame::from_slice(buf));
        for x in 0..self.tx_queue.len() {
            let b = self.tx_queue.remove(x);
            let result = self.inner.write(&b[..]);
            if result.is_err() {
                let err = result.unwrap_err();
                if err.kind() == ErrorKind::WouldBlock {
                    self.tx_queue.insert(x, b);
                }
                return Err(err)
            }

            let num_written = result.unwrap();
            total_written += num_written;
            if num_written < b.len() {
                let remainder = self.vec_from_slice(&b[(b.len() - num_written) ..b.len()]);
                self.tx_queue.insert(x, remainder);
                return Ok(total_written)
            }
        }
        Ok(total_written)
    }

    pub fn drain_rx_queue(&mut self) -> Vec<Vec<u8>> {
        let buf = self.rx_queue.clone();
        self.rx_queue = Vec::new();
        buf
    }

    fn buf_with_scratch(&mut self, buf: &[u8], len: usize) -> Vec<u8> {
        let mut new_buf = Vec::<u8>::with_capacity(self.scratch.len() + len);
        for byte in self.scratch.iter() {
            new_buf.push(*byte);
        }
        self.scratch = Vec::<u8>::new();
        for x in 0..len {
            new_buf.push(buf[x]);
        }
        new_buf
    }

    fn read_for_frame_start(&mut self, buf: &[u8], offset: &mut usize, len: usize) {
        for _ in *offset..len {
            if buf[*offset] == frame::START {
                self.buffer.push(buf[*offset]);
                self.state = FrameState::PayloadLen;
                *offset += 1;
                break;
            }
            *offset += 1;
        }
    }

    fn read_payload_len(&mut self, buf: &[u8], offset: &mut usize, len: usize) {
        for _ in *offset..len {
            self.buffer.push(buf[*offset]);
            if self.buffer.len() == 3 {
                let len = self.payload_len() + 1;
                self.buffer.reserve_exact(len);
                self.state = FrameState::Payload;
                *offset += 1;
                break;
            }
            *offset += 1;
        }
    }

    fn read_payload(&mut self, buf: &[u8], offset: &mut usize, len: usize) {
        for _ in *offset..len {
            self.buffer.push(buf[*offset]);
            if self.buffer.len() == self.payload_len() + 3 {
                self.state = FrameState::End;
                *offset += 1;
                break;
            }
            *offset += 1;
        }
    }

    fn read_for_frame_end(&mut self,
                          buf: &[u8],
                          offset: usize,
                          len: usize)
                          -> Result<Vec<u8>, ()> {
        if offset < len {
            let expected_end_byte = buf[offset];
            if expected_end_byte == frame::END {
                let mut payload = Vec::<u8>::with_capacity(self.payload_len());
                for x in 3..self.buffer.len() {
                    payload.push(self.buffer[x]);
                }

                self.state = FrameState::Start;
                self.buffer = Vec::<u8>::with_capacity(3);

                // If there is anything left in buf, we need to put it in our
                // scratch space because we're exiting here
                let mut offset = offset;
                offset += 1;
                self.scratch = Vec::<u8>::with_capacity(len - offset);
                for x in offset..len {
                    self.scratch.push(buf[x]);
                }
                return Ok(payload)
            }

            // If we're here, the frame was wrong. Maybe our fault, who knows?
            // Either way, we're going to reset and try to start again from the start byte.
            // We need to dump whatever is left in the buffer into our scratch because it
            // might be in there?
            self.state = FrameState::Start;
            self.buffer = Vec::<u8>::with_capacity(3);
            self.scratch = Vec::<u8>::with_capacity(len - offset);
            for x in offset..len {
                self.scratch.push(buf[x]);
            }
        }
        Err(())
    }

    fn payload_len(&self) -> usize {
        let mask = 0xFFFFu16;
        let len = ((self.buffer[1] as u16) << 8) & mask;
        (len | self.buffer[2] as u16) as usize
    }

    fn vec_from_slice(&self, slice: &[u8]) -> Vec<u8> {
        let mut buf = Vec::<u8>::with_capacity(slice.len());
        for byte in slice.iter() {
            buf.push(*byte);
        }
        buf
    }
}
