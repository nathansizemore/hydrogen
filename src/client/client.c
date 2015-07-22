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


#include <stdio.h>
#include <stdlib.h>


// Writer Sender<T> given to us from Rust
void *write_tx;

// Sender<T> given to us from Rust to stop the client
void *stop_tx;

//
void register_writer_tx(void *tx)
{
    write_tx = *tx;
}

//
void register_stop_tx(void *tx)
{
    stop_tx = *tx;
}
