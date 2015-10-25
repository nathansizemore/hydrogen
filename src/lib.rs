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
extern crate simple_stream;
extern crate num_cpus;
extern crate epoll;
extern crate rustc_serialize;

use std::sync::Mutex;
use std::net::ToSocketAddrs;

use types::*;

pub mod types;
pub mod socket;

mod stats;
mod epollloop;
mod resources;
mod workerthread;


/// Starts the server binding to the passed address.
/// This is a blocking call for the life of the server.
pub fn begin<T, K>(address: T, handler: Box<K>) where
    T: ToSocketAddrs + Send + 'static,
    K: EventHandler + Send + Sync + 'static {

    // Logger
    initialize_logger();

    // Data collection
    let mut data = Mutex::new(stats::GeneralData::new());
    stats::init(&mut data);

    // Begin server
    epollloop::begin(address, handler);
}

/// Initializes the global logger
fn initialize_logger() {
    let _ = fern::init_global_logger(fern::DispatchConfig {
        format: Box::new(|msg: &str, level: &log::LogLevel, _location: &log::LogLocation| {
            format!("[{}][{}] {}", time::now().strftime("%Y-%m-%d][%H:%M:%S").unwrap(), level, msg)
        }),
        output: vec![fern::OutputConfig::stdout(), fern::OutputConfig::file("/var/log/hydrogen.log")],
        level: log::LogLevelFilter::Trace,
    }, log::LogLevelFilter::Trace);

    info!("Logger initialized. \nLog file: /var/log/hydrogen.log");
}
