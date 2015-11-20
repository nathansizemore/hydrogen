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

pub enum FrameState {
    Start,
    PayloadLen,
    Payload,
    End,
    Scratch
}
