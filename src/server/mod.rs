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


extern crate rand;
extern crate epoll;
extern crate simple_stream;
extern crate num_cpus;


use std::thread;
use std::thread::JoinHandle;
use std::sync::Arc;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender, Receiver};

use self::types::*;
use self::socket::Socket;
use self::eventloop::EventLoop;
use self::resources::ResourcePool;

pub mod types;
pub mod socket;

mod eventloop;
mod resources;
mod workerthread;


pub struct FpWrapper {
    fp: EventFunctionPtr
}

impl FpWrapper {
    pub fn new(fp: EventFunctionPtr) -> FpWrapper {
        FpWrapper {
            fp: fp
        }
    }

    pub fn run(&self, sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
        (*self.fp)(sockets, socket, buffer)
    }
}


#[allow(dead_code)]
pub struct Server {
    /// Epoll event loop
    eloop_prox: EventLoop,
    /// Incoming connection listener
    conn_prox: JoinHandle<()>,
    /// On data receiver from event loop
    data_rx: Receiver<EventTuple>,
    /// Called when data is received from event loop
    fp_wrapper: Arc<FpWrapper>
}

impl Server {

    /// Creates a new server
    pub fn new(address: &str) -> Server {
        let conn_addr = address.to_string();

        // Communication channel for the event loop to bubble
        // up messages to the server receiver
        let (tx, rx): (Sender<EventTuple>, Receiver<EventTuple>) = channel();

        // Create the event loop
        let eloop = EventLoop::new(tx);

        // Channel for TcpListener to send new connections to EventLoop
        let eloop_tx = eloop.sender();

        // Start listening for incoming connections
        let listener = thread::Builder::new()
            .name("TcpListener".to_string())
            .spawn(move||{
                Server::listen(conn_addr, eloop_tx);
            }).unwrap();

        Server {
            conn_prox: listener,
            eloop_prox: eloop,
            data_rx: rx,
            fp_wrapper: Arc::new(FpWrapper::new(Box::new(Server::default_execute)))
        }
    }

    /// Listens for incoming connections and sends them to the event loop
    fn listen(address: String, eloop_tx: Sender<TcpStream>) {
        let listener = TcpListener::bind(&address[..]).unwrap();
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => { let _ = eloop_tx.send(stream); }
                Err(_) => { }
            }
        }
        drop(listener);
    }

    /// Registers the function to execute when data is received
    pub fn on_data_received(&mut self, execute: EventFunctionPtr) {
        self.fp_wrapper = Arc::new(FpWrapper::new(execute));
    }

    /// Starts the server listening to the event loop
    pub fn begin(&mut self) {
        let mut r_pool = ResourcePool::new();
        loop {
            match self.data_rx.recv() {
                Ok((sockets, socket, buff)) => {
                    let fp_wrapper = self.fp_wrapper.clone();
                    r_pool.run(fp_wrapper, sockets, socket, buff);
                }
                Err(_) => {
                    // TODO - Figure out a way to restart the event loop
                }
            }
        }
    }

    /// Default execute function
    #[allow(unused_variables)]
    fn default_execute(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
        println!("Default function executed")
    }
}
