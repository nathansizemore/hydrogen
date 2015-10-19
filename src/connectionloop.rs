// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.


use std::thread;
use std::net::{TcpStream, ToSocketAddrs, TcpListener};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    TryRecvError
};

use ipc::*;

pub struct ConnectionLoop;

impl ConnectionLoop {

    /// Returns a new ConnectionLoop
    pub fn new<T: ToSocketAddrs + Send + 'static>(address: T, epoll_tx: Sender<TcpStream>) -> ConnectionLoop {
        let prox = thread::Builder::new()
            .name("ConnectionLoop".to_string())
            .spawn(move || {
                ConnectionLoop::start(address, epoll_tx)
            }).unwrap();

        ConnectionLoop
    }

    /// Listens for incoming connections and sends them to the epoll loop
    fn start<T: ToSocketAddrs>(address: T, epoll_tx: Sender<TcpStream>) {
        let listener = TcpListener::bind(address).unwrap();
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    trace!("New connection received");
                    let _ = epoll_tx.send(stream);
                }
                Err(e) => {
                    warn!("Error encountered during TCP connection: {}", e);
                }
            }
        }
        drop(listener);
    }
}

impl Ipc for ConnectionLoop {
    fn connect(&self, tx_rx: IpcChannel) { unimplemented!() }
    fn ping(&self) { unimplemented!() }
    fn pong(&self) { unimplemented!() }
    fn kill(&self) { unimplemented!() }
    fn restart(&self) { unimplemented!() }
    fn sender(&self) -> Sender<IpcMessage> { unimplemented!() }
}
