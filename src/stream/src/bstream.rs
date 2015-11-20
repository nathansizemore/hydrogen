// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::io::{Read, Write, Error, ErrorKind};

use socket::Socket;



pub struct Bstream<T: Read + Write> {
    innder: T
}

impl Bstream {
    pub fn new<T: Read + Write>(stream: T) -> Bstream<T> {

    }
}

impl Read for Bstream {

}

impl Write for Bstream {

}
