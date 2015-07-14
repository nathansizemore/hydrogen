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


use std::sync::mpsc::Sender;


/// A trait for objects that are handles to separate processes.
/// Basic Inner Process Communication protocol
pub trait Ipc {

    /// Used to receive messages
    fn recv();

    /// Used to send messages
    fn send<T>(paylaod: T);

    /// Used to get a handle to the process' sender
    fn sender<T>(&self) -> Sender<T>;

    /// Signals it's time for the process to die. Perform any cleanup here
    fn cleanup();

    /// Terminate immediately
    fn destroy();
}
