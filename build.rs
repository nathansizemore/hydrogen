use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    println!("{}", out_dir);

    // note that there are a number of downsides to this approach, the comments
    // below detail how to improve the portability of these commands.
    // Command::new("gcc").args(&["src/client/client.c", "-c", "-fPIC", "-o"])
    //                     .arg("src/client/client.o")
                    //    .arg(&format!("{}/client.o", out_dir))
                       //.status().unwrap();
    // Command::new("ar").args(&["crus", "libclient.a", "client.o"])
    //                   .current_dir(&Path::new(&out_dir))
    //                   .status().unwrap();

    // println!("cargo:rustc-link-search=native={}", out_dir);
    // println!("cargo:rustc-link-lib=static=client");
}
