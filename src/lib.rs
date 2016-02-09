// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


#[macro_use]
extern crate log;
extern crate time;
extern crate libc;
extern crate rand;
extern crate epoll;
extern crate errno;
extern crate openssl;
extern crate num_cpus;
extern crate rustc_serialize;
extern crate simple_stream as ss;


use std::sync::Mutex;
use types::*;
use config::Config;

pub use ss::Stream;
pub use ss::{SSend, StreamShutdown};

pub mod config;
pub mod types;

mod stats;
mod server;
mod resources;
mod workerthread;


/// Starts the server using the passed `config` options.
/// This is a blocking call for the life of the server.
pub fn begin<T>(config: Config, handler: Box<T>)
    where T: EventHandler + Send + Sync + 'static
{
    let mut data = Mutex::new(stats::Stats::new());
    stats::init(&mut data);
    server::begin(config, handler);
}
