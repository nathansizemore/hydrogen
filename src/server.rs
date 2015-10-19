// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.


use std::mem;
use std::thread;
use std::thread::JoinHandle;
use std::sync::{Arc, Mutex};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Sender, Receiver};

use super::rustc_serialize::*;

use ipc::*;
use types::*;
use socket::Socket;
use epollloop::EpollLoop;
use resources::ResourcePool;
use connectionloop::ConnectionLoop;


pub struct Server {
    /// Incoming connection process
    conn_proc: Box<Ipc>,
    /// Epoll process
    epoll_proc: Box<Ipc>,
    /// Type of data handler this server has setup
    handler_type: HandlerType,
    /// If type is HandlerType::Callback, this member will be executed
    cb_ptr: *const EventFunction,
    /// If type is HandlerType::Channel, this Sender will be used
    tx: Sender<EventTuple>
}

enum HandlerType {
    Callback,
    Channel
}

impl Server {

    /// Creates a new server with an EventFunction as the DataHandler
    pub fn with_event_fn<T: ToSocketAddrs + Send + 'static>(address: T, handler: EventFunction) -> Server {
        // Channel to allow the epoll process to send messages to this process
        let (server_tx, server_rx): (Sender<EventTuple>, Receiver<EventTuple>) = channel();

        // Create the epoll loop
        let epoll_proc = EpollLoop::new(server_tx.clone());

        // Create the incoming connection thread and start listening for connections
        let conn_proc = ConnectionLoop::new(address, epoll_proc.sender());

        // Default Sender, will never be used for this type of server
        let (dtx, drx): (Sender<EventTuple>, Receiver<EventTuple>) = channel();

        Server {
            conn_proc: Box::new(conn_proc),
            epoll_proc: Box::new(epoll_proc),
            handler_type: HandlerType::Callback,
            cb_ptr: handler,
            tx: dtx
        }
    }

    /// Creates a new server with a Sender<EventTuple> as the DataHandler
    pub fn with_event_sender<T: ToSocketAddrs + Send + 'static>(address: T, handler: Sender<EventTuple>) -> Server {
        // Channel to allow the epoll process to send messages to this process
        let (server_tx, server_rx): (Sender<EventTuple>, Receiver<EventTuple>) = channel();

        // Create the epoll loop
        let epoll_proc = EpollLoop::new(server_tx.clone());

        // Create the incoming connection thread and start listening for connections
        let conn_proc = ConnectionLoop::new(address, epoll_proc.sender());

        // Default callback, will never be used for this type of server
        fn default_fn(a: SocketList, b: Socket, c: Vec<u8>) { }

        Server {
            conn_proc: Box::new(conn_proc),
            epoll_proc: Box::new(epoll_proc),
            handler_type: HandlerType::Channel,
            cb_ptr: &default_fn,
            tx: handler
        }
    }
}

// impl Server {
//
//     /// Creates a new server
//     pub fn new(handler: Box<Ipc>) -> Server {
//         let conn_addr = address.to_string();
//
//
//
//         // Create the event loop
//         let eloop = EventLoop::new(tx);
//
//         // Channel for TcpListener to send new connections to EventLoop
//         let eloop_tx = eloop.sender();
//
//         // Start listening for incoming connections
//         let listener = thread::Builder::new()
//             .name("TcpListener".to_string())
//             .spawn(move||{
//                 Server::listen(conn_addr, eloop_tx);
//             }).unwrap();
//
//         Server {
//             conn_prox: listener,
//             eloop_prox: eloop,
//             data_rx: rx,
//             fp_wrapper: Arc::new(FpWrapper::new(Box::new(Server::default_execute)))
//         }
//     }
//
//     /// Listens for incoming connections and sends them to the event loop
//     fn listen(address: String, eloop_tx: Sender<TcpStream>) {
//         let listener = TcpListener::bind(&address[..]).unwrap();
//         for stream in listener.incoming() {
//             match stream {
//                 Ok(stream) => {
//                     trace!("New connection received");
//                     let _ = eloop_tx.send(stream);
//                 }
//                 Err(e) => {
//                     warn!("Error encountered during TCP connection: {}", e);
//                 }
//             }
//         }
//         drop(listener);
//     }
//
//     /// Registers the function to execute when data is received
//     pub fn on_data_received(&mut self, execute: EventFunctionPtr) {
//         trace!("registering on_data_received handler");
//         self.fp_wrapper = Arc::new(FpWrapper::new(execute));
//     }
//
//     /// Starts the server listening to the event loop
//     pub fn begin(&mut self) {
//         trace!("server begin");
//
//         // Setup server statistics
//         let mut locked_data = Mutex::new(stats::GeneralData::new());
//         stats::init(&mut locked_data);
//         info!("data module initialized");
//
//         let mut r_pool = ResourcePool::new();
//         loop {
//             match self.data_rx.recv() {
//                 Ok((sockets, socket, buff)) => {
//                     trace!("data received, sending to resource pool");
//                     let mut fp_wrapper = self.fp_wrapper.clone();
//
//                     // Request for server statistics
//                     if buff.len() == 5 && buff[0] == 0x0D {
//                         trace!("request for server statistics");
//                         fp_wrapper = Arc::new(FpWrapper::new(Box::new(Server::request_for_server_stats)));
//                     }
//
//                     r_pool.run(fp_wrapper, sockets, socket, buff);
//                 }
//                 Err(e) => {
//                     error!("Err(e) hit on event loop Receiver: {}", e);
//                     break;
//                 }
//             };
//         }
//     }
//
//     /// Default execute function
//     #[allow(unused_variables)]
//     fn default_execute(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
//         debug!("default data handler executed...?");
//     }
//
//     /// Request for server stats
//     #[allow(unused_variables)]
//     fn request_for_server_stats(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
//         trace!("request_for_server_stats");
//
//         let sec_interval;
//         let u8_ptr = buffer.as_ptr();
//         unsafe {
//             let f32_ptr: *const f32 = mem::transmute(u8_ptr.offset(1));
//             sec_interval = *f32_ptr;
//         }
//
//         match stats::as_serialized_buffer(sec_interval) {
//             Ok(ref mut buf) => {
//                 trace!("Writing response, buf.len(): {}", buf.len());
//                 // Yeah, this is dumb...
//                 // FIXME - Find a way to accept a mutable reference to a socket, or
//                 // maybe go through and make the streams implement copy?
//                 let mut socket = socket.clone();
//                 let _ = socket.write(buf);
//             }
//             Err(_) => { }
//         };
//     }
// }
