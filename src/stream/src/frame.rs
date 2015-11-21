// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


/// Indicates start of frame
pub const START:    u8 = 0x01;
/// Indicates end of frame
pub const END:      u8 = 0x17;
/// Indicates no data
pub const EMPTY:    u8 = 0x00;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FrameState {
    Start,
    PayloadLen,
    Payload,
    End
}

pub fn from_slice(slice: &[u8]) -> Vec<u8> {
    let len = slice.len() as u16;
    let mut buf = Vec::<u8>::with_capacity(slice.len() + 4);
    buf.push(START);
    buf.push((len >> 8) as u8);
    buf.push(len as u8);
    for byte in slice.iter() {
        buf.push(*byte);
    }
    buf.push(END);
    buf
}
