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


use std::mem;
use std::thread;
use std::thread::JoinHandle;
use std::sync::{Arc, Mutex};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender, Receiver};

pub use super::rustc_serialize::*;

use self::types::*;
use self::socket::Socket;
use self::eventloop::EventLoop;
use self::resources::ResourcePool;

pub mod types;
pub mod socket;
pub mod stats;

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
                Ok(stream) => {
                    trace!("New connection received");
                    let _ = eloop_tx.send(stream);
                }
                Err(e) => {
                    warn!("Error encountered during TCP connection: {}", e);
                }
            }
        }
        drop(listener);
    }

    /// Registers the function to execute when data is received
    pub fn on_data_received(&mut self, execute: EventFunctionPtr) {
        trace!("registering on_data_received handler");
        self.fp_wrapper = Arc::new(FpWrapper::new(execute));
    }

    /// Starts the server listening to the event loop
    pub fn begin(&mut self) {
        trace!("server begin");

        // Setup server statistics
        let mut locked_data = Mutex::new(stats::GeneralData::new());
        stats::init(&mut locked_data);
        trace!("stats initialized");

        let mut r_pool = ResourcePool::new();
        loop {
            match self.data_rx.recv() {
                Ok((sockets, socket, buff)) => {
                    trace!("data received, sending to resource pool");
                    let mut fp_wrapper = self.fp_wrapper.clone();

                    // Request for server statistics
                    if buff.len() == 5 && buff[0] == 0x0D {
                        fp_wrapper = Arc::new(FpWrapper::new(Box::new(Server::request_for_server_stats)));
                    }

                    r_pool.run(fp_wrapper, sockets, socket, buff);
                }
                Err(e) => {
                    error!("Err(e) hit on event loop Receiver: {}", e);
                    break;
                }
            };
        }
    }

    /// Default execute function
    #[allow(unused_variables)]
    fn default_execute(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
        debug!("default data handler executed...?");
    }

    /// Request for server stats
    #[allow(unused_variables)]
    fn request_for_server_stats(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
        let mut sec_interval = 1.0f32;
        let u8_ptr = buffer.as_ptr();
        unsafe {
            let f32_ptr: *const f32 = mem::transmute(u8_ptr.offset(1));
            sec_interval = *f32_ptr;
        }

        match stats::as_serialized_buffer(sec_interval) {
            Ok(ref mut buf) => {
                // Yeah, this is dumb...
                // FIXME - Find a way to accept a mutable reference to a socket, or
                // maybe go through and make the streams implement copy?
                let mut socket = socket.clone();
                socket.write(buf);
            }
            Err(_) => { }
        };
    }
}
