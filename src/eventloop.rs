// Copyright 2016 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.


use std::sync::Arc;
use std::io::{self, Error};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

use ::EventHandler;
use ::Stream;
use ::types::NewConnectionQueue;


static mut epfd_ptr: *mut RawFd = 0 as *mut RawFd;


pub fn begin<T: Stream>(event_handler: Box<EventHandler<T>>, new_conns: NewConnectionQueue<T>) {

}
