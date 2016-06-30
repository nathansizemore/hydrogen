// Copyright 2016 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.



use ::EventHandler;
use ::Stream;


pub struct Handler<T: Stream> {
    pub inner: *mut EventHandler<T>
}

impl<T: Stream> Clone for Handler<T> {
    fn clone(&self) -> Handler<T> {
        unsafe {
            let same = &mut (*(*self).inner);
            Handler { inner: same }
        }
    }
}

unsafe impl<T: Stream> Send for Handler<T> {}
unsafe impl<T: Stream> Sync for Handler<T> {}
