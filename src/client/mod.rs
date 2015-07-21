// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.
//
// This Source Code Form is "Incompatible
// With Secondary Licenses", as defined by
// the Mozilla Public License, v. 2.0.


use std::str;
use std::ffi::CStr;
use std::thread;
use std::thread::JoinHandle;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    RecvError
};

use super::libc;
use super::libc::{size_t, c_void, c_int, ssize_t, c_char};


/// Connects to the provided address, (eg "123.123.123.123:3000")
pub extern fn connect(address: *const c_char) {
    let mut r_address;
    unsafe {
        r_address = CStr::from_ptr(address);
    }
    let s_address = r_address.to_bytes();
    let own_address = match str::from_utf8(s_address) {
        Ok(safe_str) => safe_str,
        Err(e) => panic!("Invalid host address")
    };

}
