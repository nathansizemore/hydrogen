// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


//! Various types used throughout the server crate


use std::sync::{Arc, Mutex};
use std::collections::LinkedList;
use std::sync::mpsc::{Sender, Receiver};

use socket::Socket;

pub trait EventHandler {
    fn on_data_received(&self, sender: Socket, sockets: SocketList, buffer: Vec<u8>);
    fn on_socket_closed(&self, id: u32);
}

/// Thread LinkedList<Socket>
pub type SocketList = Arc<Mutex<LinkedList<Socket>>>;

/// Thread safe EventHandler
pub type SafeHandler = Arc<Mutex<EventHandler>>;
