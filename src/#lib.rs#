// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


//! hydrogen is a non-blocking Edge Triggered TCP socket lib built atop [epoll][epoll-man-page]
//! with performance, concurrency, and scalability as its main priorities. It takes care of the
//! tedious connection and I/O marshalling across threads, and leaves the specifics of I/O reading
//! and writing up the consumer, through trait implementations.
//!
//! # Streams
//!
//! hydrogen manages the state of connections through [`hydrogen::Stream`][stream-trait]
//! [Trait Objects][trait-objects].
//!
//! # Events
//!
//! hydrogen reports all events to the [`hydrogen::Handler`][handler] passed during creation.
//! Interaction to `hydrogen::Stream` trait objects is made through a simple wrapper,
//! `HydrogenSocket`, to ensure thread safety.
//!
//! # Example Usage
//!
//! The following is a simple snippet using the [simple-stream][simple-stream-repo] crate to
//! provide the non-blocking I/O calls.
//!
//!
//! ```ignore
//! extern crate hydrogen;
//! extern crate simple_stream as ss;
//!
//! use hydrogen;
//! use hydrogen::{Stream as HydrogenStream, HydrogenSocket};
//! use ss::frame::Frame;
//! use ss::frame::simple::{SimpleFrame, SimpleFrameBuilder};
//! use ss::{Socket, Plain, NonBlocking, SocketOptions};
//!
//!
//! #[derive(Clone)]
//! pub struct Stream {
//!     inner: Plain<Socket, SimpleFrameBuilder>
//! }
//!
//! impl HydrogenStream for Stream {
//!     fn recv(&mut self) -> Result<Vec<Vec<u8>>, Error> {
//!         match self.inner.nb_recv() {
//!             Ok(frame_vec) => {
//!                 let mut ret_buf = Vec::<Vec<u8>>::with_capacity(frame_vec.len());
//!                 for frame in frame_vec.iter() {
//!                     ret_buf.push(frame.payload());
//!                 }
//!                 Ok(ret_buf)
//!             }
//!             Err(e) => Err(e)
//!         }
//!     }
//!
//!     fn send(&mut self, buf: &[u8]) -> Result<(), Error> {
//!         let frame = SimpleFrame::new(buf);
//!         self.inner.nb_send(&frame)
//!     }
//!
//!     fn shutdown(&mut self) -> Result<(), Error> {
//!         self.inner.shutdown()
//!     }
//! }
//! impl AsRawFd for Stream {
//!     fn as_raw_fd(&self) -> RawFd { self.inner.as_raw_fd() }
//! }
//!
//!
//! struct Server;
//! impl hydrogen::Handler for Server {
//!     fn on_server_created(&mut self, fd: RawFd) {
//!
//!     }
//!
//!     fn on_new_connection(&mut self, fd: RawFd) -> Arc<UnsafeCell<HydrogenStream>> {
//!
//!     }
//!
//!     fn on_data_received(&mut self, socket: HydrogenSocket, buffer: Vec<u8>) {
//!
//!     }
//!
//!     fn on_connection_removed(&mut self, fd: RawFd, err: Error) {
//!
//!     }
//! }
//!
//!
//! fn main() {
//!     hydrogen::begin(Server, hydrogen::Config {
//!         addr: "0.0.0.0".to_string(),
//!         port: 1337,
//!         max_threads: 8,
//!         pre_allocated: 100000
//!     });
//! }
//!
//! ```
//!
//!
//!
//! [epoll-man-page]: http://man7.org/linux/man-pages/man7/epoll.7.html
//! [stream-trait]: https://nathansizemore.github.io/hydrogen/hydrogen/trait.Stream.html
//! [trait-objects]: https://doc.rust-lang.org/book/trait-objects.html
//! [handler]: https://nathansizemore.github.io/hydrogen/hydrogen/trait.Handler.html
//! [simple-stream-repo]: https://github.com/nathansizemore/simple-stream


#[macro_use]
extern crate log;
extern crate libc;
extern crate errno;
extern crate threadpool;
extern crate simple_slab;


use std::io::Error;
use std::sync::Arc;
use std::cell::UnsafeCell;
use std::os::unix::io::{RawFd, AsRawFd};


pub use config::Config;
pub use types::HydrogenSocket;

mod types;
mod server;
mod config;


/// Trait object responsible for handling reported I/O events.
pub trait Stream : AsRawFd + Send + Sync {
    /// Called when epoll reports data is available for read.
    ///
    /// This method should read until `ErrorKind::WouldBlock` is received. At that time, all
    /// complete messages should be returned, otherwise return the std::io::Error.
    fn recv(&mut self) -> Result<Vec<Vec<u8>>, Error>;
    /// Called as the internal writer for the HydrogenSocket wrapper.
    ///
    /// This method should write until all bytes have been written or any `std::io::Error` is
    /// returned.
    fn send(&mut self, buf: &[u8]) -> Result<(), Error>;
    /// This method is called when any error, other than `ErrorKind::WouldBlock`, is returned from
    /// a `recv` or `send` call.
    fn shutdown(&mut self) -> Result<(), Error>;
}

/// Events reported to lib consumer.
pub trait Handler {
    /// This method is called once the listening RawFd has been created.
    ///
    /// It should be used to set/remove any flags on the underlying RawFd before `listen` is
    /// called on the fd.
    fn on_server_created(&mut self, fd: RawFd);
    /// This method is called whenever `accept` returns a new TCP connection.
    ///
    /// The returned trait object is added to the connection pool and the epoll interest list.
    fn on_new_connection(&mut self, fd: RawFd) -> Arc<UnsafeCell<Stream>>;
    /// This method is called whenever the `recv` call returns an Ok(_) result.
    fn on_data_received(&mut self, socket: HydrogenSocket, buf: Vec<u8>);
    /// This method is called after a stream has been removed from the connection poll and epoll
    /// interest list, with the `std::io::Error` as the reason removed.
    ///
    /// At the time of this call, the underlying fd has been shutdown and closed. No system level
    /// shutdown is needed, only application level cleanup.
    fn on_connection_removed(&mut self, fd: RawFd, err: Error);
}

/// Starts the server with the passed configuration and handler.
pub fn begin<T>(handler: Box<T>, cfg: Config)
    where T: Handler + Send + Sync + 'static
{
    server::begin(handler, cfg);
}
