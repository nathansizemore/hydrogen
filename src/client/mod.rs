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


use std::{str, ptr};
use std::ffi::{CStr, CString};
use std::thread;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Sender, Receiver};

use super::libc::{c_int, c_char};
use super::simple_stream::bstream::Bstream;


extern "C" {
    fn register_writer_tx(tx: *mut Sender<Vec<u8>>);
    fn register_stop_tx(tx: *mut Sender<()>);
}


/// Connects to the provided address, (eg "123.123.123.123:3000") and registers
/// the on data received handler
#[no_mangle]
pub extern "C" fn connect(address: *const c_char,
    handler: extern fn(*const c_char, c_int),
    on_connect_handler: extern fn(),
    on_disconnect_handler: extern fn()) -> c_int {

    let mut r_address;
    unsafe {
        r_address = CStr::from_ptr(address);
    }
    let s_address = r_address.to_bytes();
    let host_address = match str::from_utf8(s_address) {
        Ok(safe_str) => safe_str,
        Err(_) => {
            println!("Invalid host address");
            return -1 as c_int;
        }
    };

    // Create and register a way to kill this client
    let (k_tx, kill_rx): (Sender<()>, Receiver<()>) = channel();
    let kill_tx = k_tx.clone();
    let mut k_tx_ptr = Box::new(k_tx);
    unsafe {
        register_stop_tx(&mut *k_tx_ptr);
    }

    // Writer thread's channel
    let (w_tx, w_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();
    let mut w_tx_ptr = Box::new(w_tx);
    unsafe {
        register_writer_tx(&mut *w_tx_ptr);
    }

    // println!("host_address: {}", host_address);

    // let result = TcpStream::connect(host_address);
    // if result.is_err() {
    //     println!("Error connecting to {} - {}", host_address, result.unwrap_err());
    //     return -1 as c_int;
    // }
    // println!("Connected");
    // on_connect_handler();

    // let stream = result.unwrap();
    // let client = Bstream::new(stream);
    //
    // let r_client = client.clone();
    // let r_kill_tx = kill_tx.clone();
    //
    // let w_client = client.clone();
    // let w_kill_tx = kill_tx.clone();
    //
    // // Start the reader thread
    // thread::Builder::new()
    //     .name("ReaderThread".to_string())
    //     .spawn(move||{
    //         reader_thread(r_client, handler, r_kill_tx)
    //     }).unwrap();
    //
    // // Start the writer thread
    // thread::Builder::new()
    //     .name("WriterThread".to_string())
    //     .spawn(move||{
    //         writer_thread(w_rx, w_client, w_kill_tx)
    //     }).unwrap();
    //
    // // Wait for the kill signal
    // match kill_rx.recv() {
    //     Ok(_) => { }
    //     Err(e) => {
    //         println!("Error on kill channel: {}", e);
    //         return -1 as c_int;
    //     }
    // };
    // on_disconnect_handler();

    // Exit out in standard C fashion
    0 as c_int
}

/// Writes the complete contents of buffer to the server
/// Returns -1 on error
#[no_mangle]
pub extern "C" fn send_to_writer(w_tx: *mut Sender<Vec<u8>>,
                                 buffer: *const c_char,
                                 count: c_int,
                                 k_tx: *mut Sender<()>) -> c_int {
    if count < 1 {
        println!("Error - count must be greater than zero");
        return -1 as c_int;
    }

    let num_elts = count as usize;
    let mut n_buffer = Vec::<u8>::with_capacity(num_elts);
    for x in 0..num_elts as isize {
        unsafe {
            n_buffer.push(ptr::read(buffer.offset(x)) as u8);
        }
    }

    unsafe {
        match (*w_tx).send(n_buffer) {
            Ok(_) => { }
            Err(e) => {
                println!("Error sending buffer: {}", e);
                let _ = (*k_tx).send(());
                return -1 as c_int;
            }
        };
    }

    0 as c_int
}

/// Forever listens to incoming data and when a complete message is received,
/// the passed callback is hit
fn reader_thread(client: Bstream,
                 event_handler: extern fn(*const c_char, c_int),
                 kill_tx: Sender<()>) {

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
                        event_handler(c_buffer.as_ptr(), buffer.len() as c_int);
                    }).unwrap();
            }
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        };
    }
    println!("Reader thread finished");
    let _ = kill_tx.send(());
}

/// Forever listens to Receiver<Vec<u8>> waiting on messages to come in
/// Once available, blocks until the entire message has been written
fn writer_thread(rx: Receiver<Vec<u8>>, client: Bstream, kill_tx: Sender<()>) {
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
    let _ = kill_tx.send(());
}
