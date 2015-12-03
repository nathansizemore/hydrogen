// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.


extern crate gcc;

fn main() {
    if cfg!(target_os = "linux") {
        gcc::compile_library("libshim.a", &["shim.c"]);
    } else {
        gcc::compile_library("libshim.a", &["shim-osx.c"]);
    }
}
