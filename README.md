# hydrogen

Presence system


Hopeful syntax
~~~rust
fn main() {
    let mut server = Server::new("0.0.0.0:1337");
    server.on_data_recveived(on_message);
    server.begin();
}

pub fn on_message(
    // All available sockets
    sockets: Arc<Mutex<LinkedList<Socket>>>,
    // Sender of message
    socket: Socket,
    // Message
    buffer: Vec<u8>) {

    // This is ran every time data is read from any socket
}
~~~
