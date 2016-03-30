// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use openssl::ssl::SslContext;


pub struct Config {
    /// Address to bind to
    pub addr: String,
    /// Port to bind to
    pub port: u16,
    /// OpenSSL conext to use for new connections
    pub ssl: Option<SslContext>,
    /// The maximum number of threads available to hydrogen
    /// not including the two needed for new connection handling
    /// and main event loop.
    pub max_threads: usize,
    /// The amount of pre-allocated slab space for connections.
    /// This should be, roughly, the maximum amount of concurrent
    /// connections expected.
    pub pre_allocated: usize
}
