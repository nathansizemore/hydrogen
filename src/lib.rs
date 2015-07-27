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

#![allow(dead_code)]

#[cfg(any(target_os="macos", target_os="linux"))]
extern crate libc;
#[cfg(any(target_os="macos", target_os="linux"))]
extern crate rand;
#[cfg(any(target_os="macos", target_os="linux"))]
extern crate simple_stream;
#[cfg(any(target_os="macos", target_os="linux"))]
extern crate num_cpus;
#[cfg(target_os = "linux")]
extern crate epoll;

#[cfg(any(target_os="macos", target_os="linux"))]
pub mod client;
#[cfg(target_os = "linux")]
pub mod server;
