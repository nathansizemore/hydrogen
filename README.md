# hydrogen [<img src="https://travis-ci.org/nathansizemore/hydrogen.svg?branch=develop">][travis-badge]

[Documentation][docs]

hydrogen is a non-blocking socket server framework built atop [epoll][epoll-man-page].
It takes care of the tedious connection and I/O marshaling across threads, and
leaves the specifics of I/O reading and writing up the consumer, through trait
implementations.

---

## Streams

hydrogen works with `Stream` trait Objects so any custom type can be used.
[simple-stream][simple-stream-repo] was built in conjunction and offers several
stream abstractions and types including Plain and Secured streams with basic
and WebSocket framing.

## Multithreaded

hydrogen is multithreaded. It uses one thread for accepting incoming
connections, one for updating epoll reported event, and one used for
marshalling I/O into a threadpool of a user specified size.

## Slab allocation

The connection pool is managed as a slab, which means traversal times are
similar to traversing a Vector, with an insertion and removal time of O(1).


## Examples

### [simple-stream][simple-stream-repo]

``` rust
extern crate hydrogen;
extern crate simple_stream as ss;

use hydrogen;
use hydrogen::{Stream as HydrogenStream, HydrogenSocket};
use ss::frame::Frame;
use ss::frame::simple::{SimpleFrame, SimpleFrameBuilder};
use ss::{Socket, Plain, NonBlocking, SocketOptions};



// Hydrogen requires a type that implements `hydrogen::Stream`.
// We'll implement it atop the `simple-stream` crate.
#[derive(Clone)]
pub struct Stream {
    inner: Plain<Socket, SimpleFrameBuilder>
}

impl HydrogenStream for Stream {
    // This method is called when epoll reports data is available for reading.
    fn recv(&mut self) -> Result<Vec<Vec<u8>>, Error> {
        match self.inner.nb_recv() {
            Ok(frame_vec) => {
                let mut ret_buf = Vec::<Vec<u8>>::with_capacity(frame_vec.len());
                for frame in frame_vec.iter() {
                    ret_buf.push(frame.payload());
                }
                Ok(ret_buf)
            }
            Err(e) => Err(e)
        }
    }

    // This method is called when a previous attempt to write has returned `ErrorKind::WouldBlock`
    // and epoll has reported that the socket is now writable.
    fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
        let frame = SimpleFrame::new(buf);
        self.inner.nb_send(&frame)
    }

    // This method is called when connection has been reported as reset by epoll, or when any
    // `std::io::Error` has been returned.
    fn shutdown(&mut self) -> Result<(), Error> {
        self.inner.shutdown()
    }
}
impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd { self.inner.as_raw_fd() }
}


// The following will be our server that handles all reported events
struct Server;
impl hydrogen::Handler for Server {
    fn on_server_created(&mut self, fd: RawFd) {
        // Do any secific flag/option setting on the underlying listening fd.
        // This will be the fd that accepts all incoming connections.
    }

    fn on_new_connection(&mut self, fd: RawFd) -> Arc<UnsafeCell<HydrogenStream>> {
        // With the passed fd, create your type that implements `hydrogen::Stream`
        // and return it.
    }

    fn on_data_received(&mut self, socket: HydrogenSocket, buffer: Vec<u8>) {
        // Called when a complete, consumer defined, chunk of data has been read.
    }

    fn on_connection_removed(&mut self, fd: RawFd, err: Error) {
        // Called when a connection has been removed from the watch list, with the
        // `std::io::Error` as the reason removed.
    }
}


fn main() {
    hydrogen::begin(Server, hydrogen::Config {
        addr: "0.0.0.0".to_string(),
        port: 1337,
        max_threads: 8,
        pre_allocated: 100000
    });
}
```

### `std::net::TcpStream`

``` rust
extern crate hydrogen;

use std::cell::UnsafeCell;
use std::io::{Read, Write, Error, ErrorKind};
use std::net::{TcpStream, Shutdown};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::Arc;

use hydrogen::{Stream as HydrogenStream, HydrogenSocket};


pub struct Stream {
    inner: TcpStream
}

impl Stream {
    pub fn from_tcp_stream(tcp_stream: TcpStream) -> Stream {
        tcp_stream.set_nonblocking(true);
        Stream {
            inner: tcp_stream
        }
    }
}

impl HydrogenStream for Stream {
    // This method is called when epoll reports data is available for reading.
    fn recv(&mut self) -> Result<Vec<Vec<u8>>, Error> {
        let mut msgs = Vec::<Vec<u8>>::new();

        // Our socket is set to non-blocking, we need to read until
        // there is an error or the system returns WouldBlock.
        // TcpStream offers no guarantee it will return in non-blocking mode.
        // Double check OS specifics on this when using.
        // https://doc.rust-lang.org/std/io/trait.Read.html#tymethod.read
        let mut total_read = Vec::<u8>::new();
        loop {
            let mut buf = [0u8; 4098];
            let read_result = self.inner.read(&mut buf);
            if read_result.is_err() {
                let err = read_result.unwrap_err();
                if err.kind() == ErrorKind::WouldBlock {
                    break;
                }

                return Err(err);
            }

            let num_read = read_result.unwrap();
            total_read.extend_from_slice(&buf[0..num_read]);
        }

        // Multiple frames, or "msgs", could have been gathered here. Break up
        // your frames here and save remainer somewhere to come back to on the
        // next reads....
        //
        // Frame break out code goes here
        //

        msgs.push(total_read);

        return Ok(msgs);
    }

    // This method is called when a previous attempt to write has returned `ErrorKind::WouldBlock`
    // and epoll has reported that the socket is now writable.
    fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
        self.inner.write_all(buf)
    }

    // This method is called when connection has been reported as reset by epoll, or when any
    // `std::io::Error` has been returned.
    fn shutdown(&mut self) -> Result<(), Error> {
        self.inner.shutdown(Shutdown::Both)
    }
}

impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd { self.inner.as_raw_fd() }
}

// The following will be our server that handles all reported events
struct Server;
impl hydrogen::Handler for Server {
    fn on_server_created(&mut self, fd: RawFd) {
        // Do any secific flag/option setting on the underlying listening fd.
        // This will be the fd that accepts all incoming connections.
    }

    fn on_new_connection(&mut self, fd: RawFd) -> Arc<UnsafeCell<HydrogenStream>> {
        // With the passed fd, create your type that implements `hydrogen::Stream`
        // and return it.
    }

    fn on_data_received(&mut self, socket: HydrogenSocket, buffer: Vec<u8>) {
        // Called when a complete, consumer defined, chunk of data has been read.
    }

    fn on_connection_removed(&mut self, fd: RawFd, err: Error) {
        // Called when a connection has been removed from the watch list, with the
        // `std::io::Error` as the reason removed.
    }
}


fn main() {
    hydrogen::begin(Server, hydrogen::Config {
        addr: "0.0.0.0".to_string(),
        port: 1337,
        max_threads: 8,
        pre_allocated: 100000
    });
}
```

## Author

Nathan Sizemore, nathanrsizemore@gmail.com

## License

hydrogen is available under the MPL-2.0 license. See the LICENSE file for more info.



[travis-badge]: https://travis-ci.org/nathansizemore/hydrogen
[docs]: https://nathansizemore.github.io/hydrogen/hydrogen/index.html
[epoll-man-page]: http://man7.org/linux/man-pages/man7/epoll.7.html
[simple-stream-repo]: https://github.com/nathansizemore/simple-stream
