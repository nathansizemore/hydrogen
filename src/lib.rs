// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.


#[macro_use]
extern crate log;
extern crate fern;
extern crate time;
extern crate libc;
extern crate rand;
extern crate simple_stream;
extern crate num_cpus;
//#[cfg(target_os = "linux")]
extern crate epoll;
extern crate rustc_serialize;

//#[cfg(target_os = "linux")]
pub mod server;
pub mod connectionloop;
pub mod epollloop;
pub mod resources;
pub mod socket;
pub mod stats;
pub mod types;
pub mod workerthread;
pub mod ipc;



#[no_mangle]
pub extern "C" fn hydrogen_init() {
    let _ = fern::init_global_logger(fern::DispatchConfig {
        format: Box::new(|msg: &str, level: &log::LogLevel, _location: &log::LogLocation| {
            format!("[{}][{}] {}", time::now().strftime("%Y-%m-%d][%H:%M:%S").unwrap(), level, msg)
        }),
        output: vec![fern::OutputConfig::stdout(), fern::OutputConfig::file("/var/log/hydrogen.log")],
        level: log::LogLevelFilter::Trace,
    }, log::LogLevelFilter::Trace);
    info!("Initialized. \nLog file: /var/log/hydrogen.log");
}
