// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.





#[macro_use]
extern crate log;
extern crate libc;
extern crate threadpool;
extern crate simple_slab;


use std::sync::Arc;
use std::io::{self, Error};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

pub use types::Connection;

mod types;
mod eventloop;


pub trait Stream: AsRawFd {
    fn recv(&mut self) -> io::Result<Vec<Vec<u8>>>;
    fn send(&mut self, buf: &[u8]) -> io::Result<()>;
    fn shutdown(&mut self);
}

pub trait EventHandler<T: Stream> {
    fn on_server_created(&mut self, listener: &TcpListener);
    fn on_new_connection(&mut self, stream: TcpStream) -> Arc<Connection<T>>;
    fn on_data_received(&mut self, data: Vec<u8>, conn: Arc<Connection<T>>);
}
