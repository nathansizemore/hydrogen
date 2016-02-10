// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.

//! Data module
//!
//! When a request is made for current server info, the following
//! JSON is returned with information
//! {
//!     "up_time": 123456, // Seconds
//!     "num_connections": 1234,
//!     "resources": {
//!         "ram": {
//!             "bytes_used": 1234,
//!             "bytes_available": 1234
//!         },
//!         "cpu_overall": .96,
//!         "cpu_per_core": [
//!             {
//!                 "core": 0,
//!                 "using": .78 // percentage used
//!             },
//!             {
//!                 "core": 1,
//!                 "using": .23 // percentage used
//!             }
//!         ]
//!     }
//! }
//!


use std::str;
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
use std::process::Command;

use super::time;
use super::num_cpus;
use super::rustc_serialize::json;



/// This is set to point to an actual location passed in through the
/// init function. It is assumed that whatever is passed in will have a static lifetime.
/// The ownness of lifetime guarantee is on the caller of stats::init()
static mut data: *mut Mutex<Stats> = 0 as *mut Mutex<Stats>;

/// Server start time, used for calculating up_time
static mut start_time: f64 = 0 as f64;


/// Serializable struct of various performance metrics
#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct Stats {
    /// Total time since server was started
    pub up_time: u64,
    /// Current number of connections
    pub num_connections: u32,
    /// Total fds accepted through `accept()`
    pub fds_opened: u64,
    /// Total fds closed through 'close()'
    pub fds_closed: u64,
    /// Resource usage (ram/cpu)
    pub resources: ResourceData,
}

impl Stats {
    /// Creates a new `Stats` structure
    pub fn new() -> Stats {
        Stats {
            up_time: 0,
            num_connections: 0,
            fds_opened: 0,
            fds_closed: 0,
            resources: ResourceData {
                ram: RamData {
                    bytes_used: 0,
                    bytes_available: 0
                },
                cpu_overall: 0f32,
                cpu_per_core: Vec::<CpuData>::new()
            }
        }
    }
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct ResourceData {
    /// Used v Available
    pub ram: RamData,
    /// Overall CPU performance
    pub cpu_overall: f32,
    /// Collection of cpu data per core/cpu
    pub cpu_per_core: Vec<CpuData>,
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct RamData {
    /// Bytes currently in use
    pub bytes_used: u64,
    /// Bytes currently available
    pub bytes_available: u64,
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct CpuData {
    /// Core id
    pub core: u8,
    /// Amount being used
    pub using: f32
}

/// Initializes the module and sets default state
/// data_ref should represent a Mutex<Stats> structure that
/// will live for the lifetime of the application
pub fn init(data_ref: &mut Mutex<Stats>) {
    unsafe {
        data = data_ref;
        start_time = time::precise_time_s();
    }
    info!("data module initialized");
}

/// Called when a connection is added to the master list
#[inline]
pub fn conn_recv() {
    let mut guard = unsafe {
        match (*data).lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        }
    };

    let d = guard.deref_mut();
    d.num_connections += 1;
}

/// Called when a connection is removed from master list
#[inline]
pub fn conn_lost() {
    let mut guard = unsafe {
        match (*data).lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        }
    };

    let d = guard.deref_mut();
    if d.num_connections == 0 {
        warn!("Attempting to decrement clients into negative space.");
        return;
    }
    d.num_connections -= 1;
}

/// Called when a new fd is returned via `accept()`
#[inline]
pub fn fd_opened() {
    let mut guard = unsafe {
        match (*data).lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        }
    };

    let d = guard.deref_mut();
    d.fds_opened += 1;
}

/// Called when a new fd is closed via `close()`
#[inline]
pub fn fd_closed() {
    let mut guard = unsafe {
        match (*data).lock() {
            Ok(g) => g,
            Err(p) => p.into_inner()
        }
    };

    let d = guard.deref_mut();
    d.fds_closed += 1;
}

/// Returns the structure as a JSON serialized Vec<u8> with CPU data for perf_sec time
pub fn as_serialized_buffer(perf_sec: f32) -> Result<Vec<u8>, ()> {
    trace!("as_serialized_buffer");

    // We don't need to keep a lock on the struct for this operation, so we
    // are going to clone it, release the lock, and send the cloned version
    let mut d_clone;
    {
        let d_guard;
        unsafe {
            d_guard = match (*data).lock() {
                Ok(guard) => guard,
                Err(e) => {
                    error!("Failed to retrieve lock on Stats mutex");
                    error!("{}", e);
                    return Err(());
                }
            };
        }

        d_clone = d_guard.deref().clone();
    }

    // Grab CPU performance stats
    let (overall, cores) = match cpu_usage_for_secs(perf_sec) {
        Ok((o, c)) => (o, c),
        Err(_) => panic!("Unable to retrieve CPU stats...?"),
    };

    // Grab RAM stats
    let ram_stats = match get_current_ram_usage() {
        Ok(ram) => ram,
        Err(_) => panic!("Unable to retrieve RAM stats...?"),
    };

    d_clone.resources.ram = ram_stats;
    d_clone.resources.cpu_overall = overall;
    d_clone.resources.cpu_per_core = cores;
    let json_encoded = json::encode(&d_clone).unwrap();

    debug!("Server stats: {}", json_encoded);

    Ok(json_encoded.into_bytes())
}

/// Returns the CPU usage per core, and overall for the next sec second(s)
/// For example, if sec == 2, the usage results will be from time function
/// is called + 2sec.
/// This is a blocking function and will block for sec * 2 seconds. This is
/// because two delta changes at sec are needed for accurate readings
fn cpu_usage_for_secs(sec: f32) -> Result<(f32, Vec<CpuData>), ()> {
    trace!("cpu_usage_for_secs: {}", sec);

    if sec <= 0.0f32 {
        warn!("cpu_usage_for_secs - sec must be greater than zero.");
        return Err(());
    }

    // Detailed information about /proc/stat can be found here:
    // http://www.linuxhowtos.org/System/procstat.htm
    let output_1 = Command::new("cat")
                       .arg("/proc/stat")
                       .output()
                       .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

    // Sleep for sec
    let wait_time = Duration::from_millis((sec * 1000f32) as u64);
    thread::sleep(wait_time);
    let output_2 = Command::new("cat")
                       .arg("/proc/stat")
                       .output()
                       .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

    // Sleep for sec
    thread::sleep(wait_time);
    let output_3 = Command::new("cat")
                       .arg("/proc/stat")
                       .output()
                       .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

    // Unfortunately, output.stdout will be a Vec<u8> instead of a string
    // I don't really care to parse based on anything specifically, so we're
    // going to convert to a string, and then split on new lines
    let u8_buf_1 = String::from_utf8(output_1.stdout).unwrap();
    let u8_buf_2 = String::from_utf8(output_2.stdout).unwrap();
    let u8_buf_3 = String::from_utf8(output_3.stdout).unwrap();
    let stat_lines_1: Vec<&str> = u8_buf_1.split('\n').collect();
    let stat_lines_2: Vec<&str> = u8_buf_2.split('\n').collect();
    let stat_lines_3: Vec<&str> = u8_buf_3.split('\n').collect();

    trace!("stat_lines_1.len(): {}", stat_lines_1.len());
    trace!("stat_lines_2.len(): {}", stat_lines_2.len());
    trace!("stat_lines_3.len(): {}", stat_lines_3.len());

    // We only care about fields 1, 2, 3, and 4
    // +-------+------------+-------------------------------------------------------+
    // | field |    name    |   description                                         |
    // +-------+------------+-------------------------------------------------------+
    // |   1   |    user    |   Time spent in user mode                             |
    // +-------+------------+-------------------------------------------------------+
    // |   2   |    nice    |   Time spent in user mode with low priority           |
    // +-------+------------+-------------------------------------------------------+
    // |   3   |   system   |   Time spent in system mode                           |
    // +-------+------------+-------------------------------------------------------+
    // |   4   |    idle    |   Time spent in idle task                             |
    // +-------+------------+-------------------------------------------------------+
    // |   5   |   iowait   |   Time waiting for I/O to complete                    |
    // +-------+------------+-------------------------------------------------------+
    // |   6   |     irq    |   Time servicing interrupts                           |
    // +-------+------------+-------------------------------------------------------+
    // |   7   |   softirq  |   Time servicing softirqs                             |
    // +-------+------------+-------------------------------------------------------+
    // |   8   |    steal   |   Time spent in other OSes when in virtualized env    |
    // +-------+------------+-------------------------------------------------------+
    // |   9   |    quest   |   Time spent running a virtual CPU for guest OS       |
    // +-------+------------+-------------------------------------------------------+
    // |  10   | quest_nice |   Time spent running niced guest                      |
    // +-------+------------+-------------------------------------------------------+
    let mut stat_deltas = Vec::<Vec<u64>>::with_capacity(3);
    for ((line_1, line_2), line_3) in stat_lines_1.iter()
                                                  .zip(stat_lines_2.iter())
                                                  .zip(stat_lines_3.iter()) {
        let this_line_1: Vec<&str> = line_1.split_whitespace().collect();
        let this_line_2: Vec<&str> = line_2.split_whitespace().collect();
        let this_line_3: Vec<&str> = line_3.split_whitespace().collect();

        trace!("this_line_1.len(): {}", this_line_1.len());
        trace!("this_line_2.len(): {}", this_line_2.len());
        trace!("this_line_3.len(): {}", this_line_3.len());

        // Have we read all the cpu info yet?
        if !this_line_1[0].contains("cpu") {
            break;
        }

        let user_1 = i64::from_str(this_line_1[1]).unwrap();
        let user_2 = i64::from_str(this_line_2[1]).unwrap();
        let user_3 = i64::from_str(this_line_3[1]).unwrap();
        let nice_1 = i64::from_str(this_line_1[2]).unwrap();
        let nice_2 = i64::from_str(this_line_2[2]).unwrap();
        let nice_3 = i64::from_str(this_line_3[2]).unwrap();
        let system_1 = i64::from_str(this_line_1[3]).unwrap();
        let system_2 = i64::from_str(this_line_2[3]).unwrap();
        let system_3 = i64::from_str(this_line_3[3]).unwrap();
        let idle_1 = i64::from_str(this_line_1[4]).unwrap();
        let idle_2 = i64::from_str(this_line_2[4]).unwrap();
        let idle_3 = i64::from_str(this_line_3[4]).unwrap();

        trace!("user_1: {}", user_1);
        trace!("user_2: {}", user_2);
        trace!("user_3: {}", user_3);
        trace!("nice_1: {}", nice_1);
        trace!("nice_2: {}", nice_2);
        trace!("nice_3: {}", nice_3);
        trace!("system_1: {}", system_1);
        trace!("system_2: {}", system_2);
        trace!("system_3: {}", system_3);
        trace!("idle_1: {}", idle_1);
        trace!("idle_2: {}", idle_2);
        trace!("idle_3: {}", idle_3);

        let mut cpu_delta = Vec::<u64>::with_capacity(4);
        unsafe {
            cpu_delta.set_len(4);
        }

        // Eat it, arithmetic overflows...
        match (user_3 - user_2) - (user_2 - user_1) {
            x if x < 0 => cpu_delta[0] = (x * -1) as u64,
            x => cpu_delta[0] = x as u64,
        };
        match (nice_3 - nice_2) - (nice_2 - nice_1) {
            x if x < 0 => cpu_delta[1] = (x * -1) as u64,
            x => cpu_delta[1] = x as u64,
        };
        match (system_3 - system_2) - (system_2 - system_1) {
            x if x < 0 => cpu_delta[2] = (x * -1) as u64,
            x => cpu_delta[2] = x as u64,
        };
        match (idle_3 - idle_2) - (idle_2 - idle_1) {
            x if x < 0 => cpu_delta[3] = (x * -1) as u64,
            x => cpu_delta[3] = x as u64,
        };

        stat_deltas.push(cpu_delta);
    }

    // Get clock ticks per sec
    trace!("getting clk_tcks");
    let output = Command::new("getconf")
                     .arg("CLK_TCK")
                     .output()
                     .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

    trace!("getconf CLK_TCK: {}",
           String::from_utf8_lossy(&output.stdout));

    let clk_tck = u32::from_str(str::from_utf8(&output.stdout).unwrap().trim()).unwrap();

    let mut cores = Vec::<CpuData>::with_capacity(num_cpus::get());

    let mut x = 0;
    let mut overall = 0.0f32;
    for stat_delta in stat_deltas.iter() {
        let dividend = stat_delta[0] + stat_delta[1] + stat_delta[2];
        let divisor = stat_delta[0] + stat_delta[1] + stat_delta[2] + stat_delta[3];

        let usage: f32 = (dividend as f32 / divisor as f32) * (clk_tck as f32 * sec) as f32;
        if x == 0 {
            // Overall
            overall = usage;
        } else {
            // Core x
            cores.push(CpuData {
                core: x,
                using: usage
            });
        }
        x += 1;
    }

    Ok((overall, cores))
}

/// Returns a tuple of stats::Ram
fn get_current_ram_usage() -> Result<RamData, ()> {
    // Read from /proc/meminfo
    // For more info on stdout from command, see here:
    // https://github.com/torvalds/linux/blob/master/Documentation/filesystems/proc.txt#L801
    let output = Command::new("cat")
                     .arg("/proc/meminfo")
                     .output()
                     .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

    // Unfortunately, output.stdout will be a Vec<u8> instead of a string
    // I don't really care to parse based on anything specifically, so we're
    // going to convert to a string, and then split on new lines
    let u8_buf = String::from_utf8(output.stdout).unwrap();
    let meminfo_lines: Vec<&str> = u8_buf.split('\n').collect();

    trace!("meminfo_lines.len(): {}", meminfo_lines.len());

    // Current meminfo line layout
    //
    // +-----------------+-----------+-------------------+
    // |   Description   |    uint   |   uint's units    |
    // +-----------------+-----------+-------------------+
    let mut num_found = 0;
    let mut total = 0u64;
    let mut free = 0u64;
    for line in meminfo_lines.iter() {
        if line.contains("MemTotal") {
            // Total available
            let line_split: Vec<&str> = line.split_whitespace().collect();
            trace!("assigning total: {}", line_split[1]);
            // /proc/meminfo currently displays in kb
            total = u64::from_str(line_split[1]).unwrap() * 1024u64;
            num_found += 1;
        } else if line.contains("MemFree") {
            // Total available for new application
            let line_split: Vec<&str> = line.split_whitespace().collect();
            trace!("assigning free: {}", line_split[1]);
            // /proc/meminfo currently displays in kb
            free = u64::from_str(line_split[1]).unwrap() * 1024u64;
            num_found += 1;
        }

        if num_found == 2 {
            break;
        }
    }

    // Ensure we did all the wonderful string parsing correctly
    if num_found != 2 {
        error!("/proc/meminfo parsed incorrectly...?");
        return Err(());
    }

    let used = total - free;
    Ok(RamData {
        bytes_used: used,
        bytes_available: total,
    })
}
