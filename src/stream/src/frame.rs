// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


use std::fmt;


/// Indicates start of frame
pub const START:    u8 = 0x01;
/// Indicates end of frame
pub const END:      u8 = 0x17;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FrameState {
    Start,
    PayloadLen,
    Payload,
    End
}

impl fmt::Display for FrameState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            FrameState::Start => "Start".fmt(f),
            FrameState::PayloadLen => "PayloadLen".fmt(f),
            FrameState::Payload => "Payload".fmt(f),
            FrameState::End => "End".fmt(f)
        }
    }
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
