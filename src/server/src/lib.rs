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
use std::sync::{Arc, Mutex};
use std::ops::DerefMut;
use std::collections::LinkedList;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    RecvError
};

use socket::Socket;
use eventloop::EventLoop;
use resources::ResourcePool;

mod eventloop;
mod socket;
mod resources;
mod workerthread;


pub struct Server {
    /// Epoll event loop
    eloop_prox: EventLoop,
    /// Incoming connection listener
    conn_prox: JoinHandle<()>,
    /// On data receiver from event loop
    data_rx: Receiver<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>,
    /// Called when data is received from event loop
    execute: Box<Fn(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>
}


impl Server {

    /// Creates a new server
    pub fn new(address: &str) -> Server {
        let conn_addr = address.to_string();

        // Communication channel for the event loop to bubble
        // up messages to the server receiver
        let (tx, rx): (Sender<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>, Receiver<(Arc<Mutex<LinkedList<Socket>>>, Socket, Vec<u8>)>) = channel();

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
            execute: Box::new(Server::default_execute)
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

    /// Registers the function to execute when data is received
    pub fn on_data_received(&mut self,
        execute: Box<Fn(Arc<Mutex<LinkedList<Socket>>>,
                    Socket,
                    Vec<u8>)>)
    {
        self.execute = execute;
    }

    /// Starts the server listening to the event loop
    pub fn begin(&mut self) {
        let pool = ResourcePool::new();
        loop {
            match self.data_rx.recv() {
                Ok((sockets, socket, buff)) => {
                    // let fn_clone = self.execute.clone();
                    // let execute = Box::new(fn_clone);
                    // pool.run(execute);
                }
                Err(e) => {
                    // TODO - Figure out a way to restart the event loop
                }
            }
        }
    }

    /// Default execute function
    fn default_execute(sockets: Arc<Mutex<LinkedList<Socket>>>, socket: Socket, buffer: Vec<u8>) {
        println!("Default function executed")
    }
}
