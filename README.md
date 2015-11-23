# hydrogen [<img src="https://travis-ci.org/nathansizemore/hydrogen.png?branch=develop">](https://travis-ci.org/nathansizemore/hydrogen)

[Documentation](https://google.com)

Hydrogen is an extremely minimal but highly performant event based TCP
abstraction layer. The server only targets the Linux kernel, but several clients
in various languages are available.

Hydrogen uses _messages_ to send/receive from clients to server. Messages are
just a collection of 8-bit chunks and are limited to 65,535 chunks in length.

This crate includes both a `server` and `client` module.

A Rust client with FFI to C is available along with the following clients in various languages:

[hydrogen-java](https://github.com/nathansizemore/hydrogen-java)

[hydrogen-objc](https://github.com/nathansizemore/hydrogen-objc)

[hydrogen-csharp](https://github.com/nathansizemore/hydrogen-csharp)

[hydrogen-rs-ffi](https://github.com/nathansizemore/hydrogen-rs-ffi)

## Usage

##### Server

~~~rust
extern crate hydrogen;

use std::str;

use hydrogen::server::Server;
use hydrogen::server::types::*;
use hydrogen::server::socket::Socket;

fn main() {
    // Must initialize before using
    hydrogen::hydrogen_init();

    // Start the server
    let mut server = Server::new("0.0.0.0:1337");
    server.on_data_received(Box::new(on_data_received));
    server.begin();
}

pub fn on_data_received(sockets: StreamList, socket: Socket, buffer: Vec<u8>) {
    // This is currently needed because rustc doesn't know that everything here
    // is currently owned by this function.
    // This should be fixed once Copy is being derived for Socket struct
    let mut s_socket = socket.clone();

    // This will always evaluate to true in this example case
    let request = str::from_utf8(buffer).unwrap();
    if request == "ping" {
        let response = "pong".to_string().into_bytes();
        match s_socket.write(&response) {
            Ok(_) => { }
            Err(e) => println!("Error on write: {}", e)
        };
    }
}
~~~

##### Client

~~~rust
// Example coming soon
~~~

## Server Monitoring

The server comes with the ability to monitor various things related to machine
health within the next xSeconds with a simple message.

##### Statistics Request Framing
~~~
0                   1                   2                   3                   4
0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Action     |                   Number of Seconds Requested                 |
+-------------------------------------------------------------------------------+

Action:     8 bits
            Must be the actual byte, 0x0D

Seconds:    32 bits
            Interpreted as a 32-bit floating point. Representing the number
            of seconds the reading will span
~~~

The response will be a JSON encoded buffer, with the same structure as the
following example.

~~~javascript
{
    // Total time server has been alive
    "up_time": 123456,
    // Total bytes received during up_time
    "bytes_recv": 123456,
    // Total bytes sent during up_time
    "bytes_sent": 123456,
    // Total messages received during up_time
    "msg_recv": 1234,
    // Total messages sent during up_time
    "msg_sent": 1234,
    // Number of currently connected clients
    "num_clients": 1234,

    // Machine health
    "resources": {
         "ram": {
             // RAM in use reported by /proc/meminfo
             "bytes_used": 1234,
             // Total RAM available on machine
             "bytes_available": 1234
         },
         // Average CPU load during xSeconds across all cpus/cores
         "cpu_overall": 0.52,
         // Collection of usage on each core
         "cpu_per_core": [
             {
                 // Core id
                 "core": 0,
                 // Percentage of capacity used during xSeconds
                 "using": 0.78
             },
             {
                 // Core id
                 "core": 1,
                 // Percentage of capacity used during xSeconds
                 "using": 0.23
             }
         ]
    }
}
~~~

## Benchmarks
Currently, the highest load achieved has been 70k concurrent connections
with a 50k msg/sec. It is capable of much higher loads, but I just haven't had
the time to build out a testing environment capable of hitting the server with
millions of clients.


## Disclaimer
This is not quite ready for real world use. Use at your own risk, or whatever...
