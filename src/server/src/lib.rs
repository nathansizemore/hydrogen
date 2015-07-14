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


extern crate util;


use std::thread;
use std::thread::JoinHandle;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError
};

use eventloop::EventLoop;

mod eventloop;



pub struct Server {
    /// Epoll event loop
    eloop_prox: EventLoop,
    /// Incoming connection listener
    conn_prox: JoinHandle<()>
}


impl Server {

    /// Creates a new server
    pub fn new(address: &str) -> Server {
        let eloop = EventLoop::new();
        let conn_addr = address.to_string();
        let eloop_tx = eloop.sender();
        let listener = thread::Builder::new()
            .name("TcpListener".to_string())
            .spawn(move||{
                Server::listen(conn_addr, eloop_tx);
            }).unwrap();

        Server {
            conn_prox: listener,
            eloop_prox: eloop
        }
    }

    /// Listens for incoming connections and sends them to the event loop
    fn listen(address: String, eloop_tx: Sender<TcpStream>) {
        let listener = TcpListener::bind(&address[..]).unwrap();
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => { eloop_tx.send(stream); }
                Err(_) => { }
            }
        }
        drop(listener);
    }
}







#[test]
fn it_works() {
}
