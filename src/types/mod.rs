// Copyright 2016 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.


use std::sync::{Arc, Mutex};
use std::io::{self, Error};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

use ::Stream;


pub use self::connection::Connection;

mod connection;


pub type NewConnectionQueue<T: Stream> = Arc<Mutex<Vec<Connection<T>>>>;
