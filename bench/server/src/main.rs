// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


#[macro_use]
extern crate log;
extern crate libc;
extern crate env_logger;
extern crate hydrogen;
extern crate simple_stream as ss;


use std::mem;
use std::io::Error;
use std::sync::Arc;
use std::cell::UnsafeCell;
use std::os::unix::io::{RawFd, AsRawFd};

use hydrogen::{Stream as HydrogenStream, HydrogenSocket};
use ss::frame::Frame;
use ss::frame::simple::{SimpleFrame, SimpleFrameBuilder};
use ss::{Socket, Plain, NonBlocking, SocketOptions};



struct Stream {
    inner: Plain<Socket, SimpleFrameBuilder>
}

impl HydrogenStream for Stream {
    fn recv(&mut self) -> Result<Vec<Vec<u8>>, Error> {
        match self.inner.nb_recv() {
            Ok(frame_vec) => {
                let mut ret_buf = Vec::<Vec<u8>>::with_capacity(frame_vec.len());
                for frame in frame_vec.iter() {
                    ret_buf.push(frame.payload());
                }
                Ok(ret_buf)
            }
            Err(e) => Err(e)
        }
    }
    fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
        let frame = SimpleFrame::new(buf);
        self.inner.nb_send(&frame)
    }
    fn shutdown(&mut self) -> Result<(), Error> {
        self.inner.shutdown()
    }
}

impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd { self.inner.as_raw_fd() }
}

struct Server;
impl hydrogen::Handler for Server {
    fn on_server_created(&mut self, fd: RawFd) {
        let mut socket = Socket::new(fd);
        let _ = socket.set_reuseaddr(true);
    }

    fn on_new_connection(&mut self, fd: RawFd) -> Arc<UnsafeCell<hydrogen::Stream>> {
        let mut socket = Socket::new(fd);
        let _ = socket.set_nonblocking();
        let _ = socket.set_keepalive(true);

        unsafe {
            let opt = 1;
            let _ = libc::setsockopt(socket.as_raw_fd(),
                                              libc::IPPROTO_TCP,
                                              libc::TCP_NODELAY,
                                              &opt as *const _ as *const libc::c_void,
                                              mem::size_of::<libc::c_int>() as u32);
        }

        let plain_stream = Plain::<Socket, SimpleFrameBuilder>::new(socket);
        let stream = Stream {
            inner: plain_stream
        };

        Arc::new(UnsafeCell::new(stream))
    }

    #[allow(unused_variables)]
    fn on_data_received(&mut self, socket: HydrogenSocket, buf: Vec<u8>) {
        let mut pong = [0u8; 4];
        pong[0] = 'p' as u8;
        pong[1] = 'o' as u8;
        pong[2] = 'n' as u8;
        pong[3] = 'g' as u8;

        socket.send(&pong[..]);
    }

    #[allow(unused_variables)]
    fn on_connection_removed(&mut self, fd: RawFd, err: Error) { }
}

fn main() {
    env_logger::init().unwrap();
    hydrogen::begin(Box::new(Server), hydrogen::Config {
        addr: "127.0.0.1".to_string(),
        port: 1337,
        max_threads: 2,
        pre_allocated: 100
    });
}
