// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the
// terms of the Mozilla Public License, v.
// 2.0. If a copy of the MPL was not
// distributed with this file, You can
// obtain one at
// http://mozilla.org/MPL/2.0/.


use std::sync::mpsc::{channel, Sender, Receiver};


pub enum IpcMessage {
    Ping,
    Pong,
    Kill,
    Restart
}

pub type IpcChannel = (Sender<IpcMessage>, Receiver<IpcMessage>);

pub trait Ipc {
    fn connect(&self, tx_rx: IpcChannel);
    fn ipc_sender(&self) -> Sender<IpcMessage>;
    fn ping(&self);
    fn pong(&self);
    fn kill(&self);
    fn restart(&self);
}
