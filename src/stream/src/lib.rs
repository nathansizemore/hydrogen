// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


extern crate libc;

use std::os::unix::io::AsRawFd;
use std::io::{Read, Write, Error};

pub mod frame;
pub mod socket;
pub mod nbstream;

pub trait Stream<T: Read + Write + AsRawFd>: Sized {
    fn new(stream: T) -> Result<Self, Error>;
    fn recv(&mut self) -> Result<Vec<u8>, Error>;
    fn send(&mut self, buf: &[u8]) -> Result<usize, Error>;
}
