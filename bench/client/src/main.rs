// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


#[macro_use]
extern crate log;
extern crate time;
extern crate env_logger;
extern crate simple_stream as ss;


use std::thread;
use std::net::TcpStream;
use std::os::unix::io::IntoRawFd;

use time::Timespec;
use ss::frame::simple::{SimpleFrame, SimpleFrameBuilder};
use ss::{Socket, Plain, Blocking};


const TOTAL_MESSAGES: usize = 100000;

static mut start_time: *mut Timespec = 0 as *mut Timespec;


#[derive(Clone)]
struct Client {
    stream: Plain<Socket, SimpleFrameBuilder>
}

fn main() {
    env_logger::init().unwrap();

    let tcp_stream = TcpStream::connect("127.0.0.1:1337").unwrap();
    let fd = tcp_stream.into_raw_fd();
    let socket = Socket::new(fd);
    let client = Client {
        stream: Plain::<Socket, SimpleFrameBuilder>::new(socket)
    };

    let client_clone = client.clone();
    let rx_thread = thread::spawn(move || reader_thread(client_clone));
    thread::spawn(move || writer_thread(client));
    let _ = rx_thread.join();
}

fn reader_thread(mut client: Client) {
    let mut frames_read: usize = 0;
    while frames_read < TOTAL_MESSAGES {
        match client.stream.b_recv() {
            Ok(_) => frames_read += 1,
            Err(e) => {
                error!("During recv: {}", e);
                return;
            }
        }
    }

    let end_time = time::get_time();
    unsafe {
        info!("{} frames sent in {}", TOTAL_MESSAGES, (end_time - *start_time));
    }
}

fn writer_thread(mut client: Client) {
    let mut ping = [0u8; 4];
    ping[0] = 'p' as u8;
    ping[1] = 'i' as u8;
    ping[2] = 'n' as u8;
    ping[3] = 'g' as u8;

    let frame = SimpleFrame::new(&ping[..]);

    let t_spec = time::get_time();
    unsafe {
        start_time = Box::into_raw(Box::new(t_spec));
    }

    for _ in 0..TOTAL_MESSAGES {
        match client.stream.b_send(&frame) {
            Ok(_) => { }
            Err(e) => {
                error!("During send: {}", e);
                return;
            }
        }
    }
}
