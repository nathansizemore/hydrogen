# hydrogen [<img src="https://travis-ci.org/nathansizemore/hydrogen.png?branch=develop">](https://travis-ci.org/nathansizemore/hydrogen)

hydrogen is a minimal wrapper around epoll for building scalable asynchronous socket servers.

## Usage

~~~rust
#[macro_use]
extern crate log;
extern crate fern;
extern crate hydrogen;

use hydrogen::types::*;
use hydrogen::nbstream::Nbstream;


struct Server;
impl EventHandler for Server {
    fn on_data_received(&mut self, stream: Nbstream, buffer: Vec<u8>) {
        // Do stuff with received data
    }

    #[allow(unused_variables)]
    fn on_stream_closed(&mut self, id: String) {
        // Do any application cleanup required on connection close
    }
}

fn main() {
    let server = Server;
    let handler = Box::new(server);
    hydrogen::begin(hydrogen::config::Config {
        // Address to bind to
        addr: "0.0.0.0".to_string(),
        // Port to listen on
        port: 1337,
        // Number of worker threads used to handle read events
        workers: 1,
        // Use TLS
        ssl: false,
        // Log level
        log_level: log::LogLevelFilter::Trace
    }, handler);
}
~~~

hydrogen is still under active development and the following features still need implemented:

* SSL
* Non-global logger
* Optimization passes
* Benchmarks
