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


use std::str;
use std::ffi::{CStr, CString};
use std::thread;
use std::thread::JoinHandle;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{
    channel,
    Sender,
    Receiver,
    RecvError
};

use super::libc;
use super::libc::{size_t, c_void, c_int, ssize_t, c_char};
use super::simple_stream::bstream::Bstream;


extern "C" {
    fn register_writer_tx(tx: *mut Sender<Vec<u8>>);
    fn register_stop_tx(tx: *mut Sender<()>);
}



/// Connects to the provided address, (eg "123.123.123.123:3000")
pub extern "C" fn connect(address: *const c_char, handler: extern fn(*const c_char)) -> c_int {
    let mut r_address;
    unsafe {
        r_address = CStr::from_ptr(address);
    }
    let s_address = r_address.to_bytes();
    let host_address = match str::from_utf8(s_address) {
        Ok(safe_str) => safe_str,
        Err(e) => {
            println!("Invalid host address");
            return -1 as c_int;
        }
    };

    // Create and register a way to kill this client
    let (kill_tx, kill_rx): (Sender<()>, Receiver<()>) = channel();
    let mut k_tx_ptr = Box::new(kill_tx);
    unsafe {
        register_stop_tx(&mut *k_tx_ptr);
    }

    // Writer thread's channel
    let (w_tx, w_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();
    let mut w_tx_ptr = Box::new(w_tx);
    unsafe {
        register_writer_tx(&mut *w_tx_ptr);
    }

    let result = TcpStream::connect(host_address);
    if result.is_err() {
        println!("Error connecting to {} - {}", host_address, result.unwrap_err());
        return -1 as c_int;
    }

    let stream = result.unwrap();
    let client = Bstream::new(stream);
    let r_client = client.clone();
    let w_client = client.clone();

    // Start the reader thread
    thread::Builder::new()
        .name("ReaderThread".to_string())
        .spawn(move||{
            reader_thread(r_client, handler)
        }).unwrap();

    // Start the writer thread
    thread::Builder::new()
        .name("WriterThread".to_string())
        .spawn(move||{
            writer_thread(w_rx, w_client)
        }).unwrap();

    // Wait for the kill signal
    match kill_rx.recv() {
        Ok(_) => { }
        Err(e) => {
            println!("Error on kill channel: {}", e);
            return -1 as c_int;
        }
    };

    // Exit out in standard C fashion
    0 as c_int
}

/// Writes the complete contents of buffer to the server
/// Returns -1 on error
pub extern "C" fn send_to_writer(w_tx: *mut c_void, buffer: *const c_char) -> c_int {
    unimplemented!()
}

/// Forever listens to incoming data and when a complete message is received,
/// the passed callback is hit
fn reader_thread(client: Bstream, event_handler: extern fn(*const c_char)) {
    let mut reader = client.clone();
    loop {
        match reader.read() {
            Ok(buffer) => {
                // Launch the handler in a new thread
                thread::Builder::new()
                    .name("Reader-Worker".to_string())
                    .spawn(move||{
                        let slice = &buffer[..];
                        let c_buffer = CString::new(slice).unwrap();
                        event_handler(c_buffer.as_ptr());
                    }).unwrap();
            }
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        };
    }
    println!("Reader thread finished");
}

/// Forever listens to Receiver<Vec<u8>> waiting on messages to come in
/// Once available, blocks until the entire message has been written
fn writer_thread(rx: Receiver<Vec<u8>>, client: Bstream) {
    let mut writer = client.clone();
    loop {
        match rx.recv() {
            Ok(ref mut buffer) => {
                match writer.write(buffer) {
                    Ok(_) => { }
                    Err(e) => {
                        println!("Error: {}", e);
                        break;
                    }
                };
            }
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        };
    }
    println!("Writer thread finished");
}
