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


extern crate hydrogen;

use hydrogen::server::Server;
use hydrogen::server::types::*;
use hydrogen::server::socket::Socket;

fn main() {
    let mut server = Server::new("0.0.0.0:1337");
    server.on_data_received(Box::new(on_data_received));
    server.begin();
}

pub fn on_data_received(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
    println!("on_data_received hit")
}
