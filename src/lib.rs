// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


#[macro_use]
extern crate log;
extern crate fern;
extern crate time;
extern crate libc;
extern crate rand;
extern crate epoll;
extern crate errno;
extern crate openssl;
extern crate num_cpus;
extern crate rustc_serialize;


use std::sync::Mutex;
use types::*;
use config::Config;

pub use stream::frame;
pub use stream::Stream;

pub mod stream;
pub mod config;
pub mod types;

mod stats;
mod server;
mod resources;
mod workerthread;


/// Starts the server binding to the passed address.
/// This is a blocking call for the life of the server.
pub fn begin<T>(config: Config, handler: Box<T>)
    where T: EventHandler + Send + Sync + 'static
{

    initialize_logger();

    // Data collection
    let mut data = Mutex::new(stats::Stats::new());
    stats::init(&mut data);

    // Begin server
    server::begin(config, handler);
}

/// Initializes the global logger
fn initialize_logger() {
    let _ = fern::init_global_logger(fern::DispatchConfig {
                                         format: Box::new(|msg: &str,
                                                           level: &log::LogLevel,
                                                           _location: &log::LogLocation| {
                                             format!("[{}][{}] {}",
                                                     time::now()
                                                         .strftime("%Y-%m-%d][%H:%M:%S")
                                                         .unwrap(),
                                                     level,
                                                     msg)
                                         }),
                                         output: vec![fern::OutputConfig::stdout(),
                                                      fern::OutputConfig::file("/var/log/hydrogen\
                                                                                .log")],
                                         level: log::LogLevelFilter::Trace,
                                     },
                                     log::LogLevelFilter::Trace);
}
