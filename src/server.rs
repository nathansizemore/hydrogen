// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::mem;
use std::thread;
use std::thread::JoinHandle;
use std::sync::{Arc, Mutex};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Sender, Receiver};

use super::rustc_serialize::*;

use stats;
use ipc::*;
use types::*;
use socket::Socket;
use epollloop::EpollLoop;
use resources::ResourcePool;
use connectionloop::ConnectionLoop;





/// Creates a new hydrogen server with an EventFunction as the message received handler
pub fn with_event_fn<T: ToSocketAddrs + Send + 'static>(address: T, handler: EventFunction) {
    // Create the epoll loop
    let epoll_proc = EpollLoop::with_event_fn(handler);

    // Create the incoming connection thread and start listening for connections
    let conn_proc = ConnectionLoop::new(address, epoll_proc.sender());

    // Default Sender, will never be used for this type of server
    let (dtx, drx): (Sender<EventTuple>, Receiver<EventTuple>) = channel();

    Server {
        conn_proc: Box::new(conn_proc),
        epoll_proc: Box::new(epoll_proc),
        handler_type: HandlerType::Callback,
        cb_ptr: handler,
        tx: dtx,
        rx: server_rx
    }
}

/// Creates a new hydrogen server with a Channel<EventTuple> as the message received handler
pub fn with_event_channel<T: ToSocketAddrs + Send + 'static>(address: T,
                                                             handler: Sender<EventTuple>) {
    let _ = 8u32;

}










pub struct Server {
    /// Incoming connection process
    conn_proc: Box<Ipc>,
    /// Epoll process
    epoll_proc: Box<Ipc>,
    /// Type of data handler this server has setup
    handler_type: HandlerType,
    /// If type is HandlerType::Callback, this member will be executed
    cb_ptr: EventFunction,
    /// If type is HandlerType::Channel, this Sender will be used
    tx: Sender<EventTuple>,
    /// When a message has been received from epoll, it comes through this channel
    rx: Receiver<EventTuple>
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
            tx: dtx,
            rx: server_rx
        }
    }

    /// Creates a new server with a Sender<EventTuple> as the DataHandler
    pub fn with_event_channel<T: ToSocketAddrs + Send + 'static>(address: T, handler: Sender<EventTuple>) -> Server {
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
            cb_ptr: Arc::new(default_fn),
            tx: handler,
            rx: server_rx
        }
    }

    /// Starts the server listening to the event loop
    pub fn start(&mut self) {
        // Get a resource pool
        let mut rsrc_pool = ResourcePool::new();

        // Setup server statistics
        let mut locked_data = Mutex::new(stats::GeneralData::new());
        stats::init(&mut locked_data);

        // Stats response function wrapper to replace self.cb_ptr when
        // the "get stats" request has been received
        let stats_response = Arc::new(Server::request_for_server_stats);
        loop {
            match self.rx.recv() {
                Ok((sockets, socket, buff)) => {
                    let task = self.cb_ptr.clone();
                    if buff.len() == 5 && buff[0] == 0x0D {
                        trace!("request for server statistics");
                        task = stats_response.clone();
                    }
                    rsrc_pool.run(task, sockets, socket, buff);
                }
                Err(e) => {
                    error!("Err(e) hit on event loop Receiver: {}", e);
                    break;
                }
            };
        }
        trace!("Server rx from epoll events closed");
    }

    /// Request for server stats
    #[allow(unused_variables)]
    fn request_for_server_stats(sockets: SocketList, socket: Socket, buffer: Vec<u8>) {
        trace!("request_for_server_stats");

        let sec_interval;
        let u8_ptr = buffer.as_ptr();
        unsafe {
            let f32_ptr: *const f32 = mem::transmute(u8_ptr.offset(1));
            sec_interval = *f32_ptr;
        }

        match stats::as_serialized_buffer(sec_interval) {
            Ok(ref mut buf) => {
                trace!("Writing response, buf.len(): {}", buf.len());
                // Yeah, this is dumb...
                // FIXME - Find a way to accept a mutable reference to a socket, or
                // maybe go through and make the streams implement copy?
                let mut socket = socket.clone();
                let _ = socket.write(buf);
            }
            Err(_) => { }
        };
    }
}
