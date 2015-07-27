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


// Rust function prototypes
int send_to_writer(void *w_tx, const char *buffer, const int count, void *k_tx);


// Writer Sender<T> given to us from Rust
void *write_tx;

// Sender<T> given to us from Rust to stop the client
void *stop_tx;


// Registers the address of Rust's Sender<T> used to signal the write
// thread there is a message to send
void register_writer_tx(void *tx)
{
    printf("%s\n", "C.register_writer_tx");
    write_tx = tx;
}

// Registers the address of Rust's Sender<T> used to signal the lib
// to disconnect and exit
void register_stop_tx(void *tx)
{
    printf("%s\n", "C.register_stop_tx");
    stop_tx = tx;
}

// Calls Rust
void write(const char *buffer, const int count)
{
    int result = send_to_writer(write_tx, buffer, count, stop_tx);
    if (result == -1)
    {
        // TODO - Determine if stdout msgs should report here or in Rust
    }
}
