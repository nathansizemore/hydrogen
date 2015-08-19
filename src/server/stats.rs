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


//! Data module
//!
//! When a request is made for current server info, the following
//! JSON is returned with information
//! {
//!     "up_time": 123456, // Seconds
//!     "bytes_recv": 123456,
//!     "bytes_sent": 123456,
//!     "msg_recv": 1234,
//!     "msg_sent": 1234,
//!     "num_clients": 1234,
//!     "resources": {
//!         "ram": {
//!             "kernel": {
//!                 "bytes_used": 1234,
//!                 "bytes_available": 1234
//!             },
//!             "user_space": {
//!                 "bytes_used": 1234,
//!                 "bytes_available": 1234
//!             }
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
use std::ops::{Deref, DerefMut};
use std::sync::Mutex;
use std::process::Command;

use super::super::time;
use super::super::num_cpus;
use super::super::rustc_serialize::json;


// Global mutable state, ftw!

// Server statistics collection
static mut data: *mut Mutex<GeneralData> = 0 as *mut Mutex<GeneralData>;

// Server start time, used for calculating up_time
static mut start_time: f64 = 0 as f64;


/// Serializable struct of various performance metrics
#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct GeneralData {
    /// Total time since server was started
    up_time: u64,
    /// Total bytes received
    bytes_recv: u64,
    /// Total bytes sent
    bytes_sent: u64,
    /// Total messages received
    msg_recv: u64,
    /// Total messages sent
    msg_sent: u64,
    /// Current number of connected clients
    num_clients: u32,
    /// Resource usage (ram/cpu)
    resources: ResourceData
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct ResourceData {
    /// Used v Available
    pub ram: Ram,
    /// Overall CPU performance
    pub cpu_overall: f32,
    /// Collection of cpu data per core/cpu
    pub cpu_per_core: Vec<CpuData>
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct Ram {
    /// Kernel space
    pub kernel: RamData,
    /// User space
    pub user_space: RamData
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct RamData {
    /// Bytes currently in use
    pub bytes_used: u64,
    /// Bytes currently available
    pub bytes_available: u64
}

#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct CpuData {
    /// Core id
    core: u8,
    /// Amount being used
    using: f32
}


impl GeneralData {

    /// Returns a new GeneralData
    pub fn new() -> GeneralData {
        GeneralData {
            up_time: 0u64,
            bytes_recv: 0u64,
            bytes_sent: 0u64,
            msg_recv: 0u64,
            msg_sent: 0u64,
            num_clients: 0u32,
            resources: ResourceData::new()
        }
    }

    /// Increments num_clients
    pub fn conn_recv(&mut self) {
        self.num_clients += 1;
    }

    /// Decrements num_clients
    pub fn conn_lost(&mut self) {
        if self.num_clients == 0 {
            warn!("Attempting to decrement clients into negative space.");
            return;
        }
        self.num_clients -= 1;
    }

    /// Sets new resource data
    pub fn set_resource_data(&mut self, new_data: ResourceData) {
        self.resources = new_data;
    }
}

impl ResourceData {

    /// Returns a new ResourceData
    pub fn new() -> ResourceData {
        let mut temp = ResourceData {
            ram: Ram {
                kernel: RamData {
                    bytes_used: 0u64,
                    bytes_available: 0u64
                },
                user_space: RamData {
                    bytes_used: 0u64,
                    bytes_available: 0u64
                }
            },
            cpu_overall: 0.0f32,
            cpu_per_core: Vec::new()
        };

        let num = num_cpus::get();
        temp.set_num_cpus(num);
        temp
    }

    /// Sets the number of cpus
    pub fn set_num_cpus(&mut self, num: usize) {
        self.cpu_per_core = Vec::<CpuData>::with_capacity(num as usize);
        for x in 0..num {
            self.cpu_per_core.push(CpuData::new(x));
        }
    }
}

impl RamData {

    /// Returns a new RamData
    pub fn new() -> RamData {
        RamData {
            bytes_used: 0u64,
            bytes_available: 0u64
        }
    }
}

impl CpuData {

    /// Returns a new CpuData
    pub fn new(id: usize) -> CpuData {
        CpuData {
            core: id as u8,
            using: 0.0f32
        }
    }

    /// Sets the core usage
    pub fn set_usage(&mut self, usage: f32) {
        self.using = usage;
    }
}


/// Initializes the module and sets default state
/// data_ref should represent a Mutex<GeneralData> structure that
/// will live for the lifetime of the application
pub fn init(data_ref: &mut Mutex<GeneralData>) {
    unsafe {
        data = data_ref;
        start_time = time::precise_time_s();
    }
}

/// Increments num_clients
#[inline]
pub fn conn_recv() {
    let mut d_guard;
    unsafe {
        match (*data).lock() {
            Ok(guard) => d_guard = guard,
            Err(e) => {
                error!("Failed to retrieve lock on GeneralData mutex");
                error!("{}", e);
                return;
            }
        };
    }

    let d = d_guard.deref_mut();
    d.num_clients += 1;
}

/// Decrements num_clients
#[inline]
pub fn conn_lost() {
    let mut d_guard;
    unsafe {
        match (*data).lock() {
            Ok(guard) => d_guard = guard,
            Err(e) => {
                error!("Failed to retrieve lock on GeneralData mutex");
                error!("{}", e);
                return;
            }
        }
    }

    let d = d_guard.deref_mut();
    if d.num_clients == 0 {
        warn!("Attempting to decrement clients into negative space.");
        return;
    }
    d.num_clients -= 1;
}

/// Increments the total number of messages received
#[inline]
pub fn msg_recv() {
    trace!("msg_recv");

    let mut d_guard;
    unsafe {
        match (*data).lock() {
            Ok(guard) => d_guard = guard,
            Err(e) => {
                error!("Failed to retrieve lock on GeneralData mutex");
                error!("{}", e);
                return;
            }
        }
    }

    let d = d_guard.deref_mut();
    d.msg_recv += 1;
}

/// Adds amount to the total number of bytes received
#[inline]
pub fn bytes_recv(amount: usize) {
    trace!("bytes_recv: {}", amount);

    let mut d_guard;
    unsafe {
        match (*data).lock() {
            Ok(guard) => d_guard = guard,
            Err(e) => {
                error!("Failed to retrieve lock on GeneralData mutex");
                error!("{}", e);
                return;
            }
        }
    }

    let d = d_guard.deref_mut();
    d.bytes_recv += amount as u64;
}

/// Adds amount to the total number of bytes sent
#[inline]
pub fn bytes_sent(amount: usize) {
    let mut d_guard;
    unsafe {
        match (*data).lock() {
            Ok(guard) => d_guard = guard,
            Err(e) => {
                error!("Failed to retrieve lock on GeneralData mutex");
                error!("{}", e);
                return;
            }
        }
    }

    let d = d_guard.deref_mut();
    d.bytes_sent += amount as u64;
}

/// Adds amount to the total number of messages sent
#[inline]
pub fn msg_sent() {
    let mut d_guard;
    unsafe {
        match (*data).lock() {
            Ok(guard) => d_guard = guard,
            Err(e) => {
                error!("Failed to retrieve lock on GeneralData mutex");
                error!("{}", e);
                return;
            }
        }
    }

    let d = d_guard.deref_mut();
    d.msg_sent += 1;
}

/// Returns the structure as a JSON serialized Vec<u8> with CPU data for perf_sec time
pub fn as_serialized_buffer(perf_sec: f32) -> Result<Vec<u8>, ()> {
    trace!("as_serialized_buffer");

    // We don't need to keep a lock on the struct for this operation, so we
    // are going to clone it, release the lock, and send the cloned version
    let mut d_clone;
    {
        let mut d_guard;
        unsafe {
            match (*data).lock() {
                Ok(guard) => d_guard = guard,
                Err(e) => {
                    error!("Failed to retrieve lock on GeneralData mutex");
                    error!("{}", e);
                    return Err(());
                }
            }
        }

        d_clone = d_guard.deref().clone();
    }

    // Grab CPU performance stats
    let (overall, cores) = match cpu_usage_for_secs(perf_sec) {
        Ok((o, c)) => (o, c),
        Err(_) => panic!("Unable to retrieve CPU stats...?")
    };

    // Grab RAM stats
    let ram_stats = match get_current_ram_usage() {
        Ok(ram) => ram,
        Err(_) => panic!("Unable to retrieve RAM stats...?")
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
    let output_1 = Command::new("cat").arg("/proc/stat")
        .output()
        .unwrap_or_else(|e| { panic!("failed to execute process: {}", e) });

    trace!("output_1: {}", String::from_utf8_lossy(&output_1.stdout));

    // Sleep for sec
    thread::sleep_ms((sec * 1000.0f32) as u32);
    let output_2 = Command::new("cat").arg("/proc/stat")
        .output()
        .unwrap_or_else(|e| { panic!("failed to execute process: {}", e) });

    trace!("output_2: {}", String::from_utf8_lossy(&output_2.stdout));

    // Sleep for sec
    thread::sleep_ms((sec * 1000.0f32) as u32);
    let output_3 = Command::new("cat").arg("/proc/stat")
        .output()
        .unwrap_or_else(|e| { panic!("failed to execute process: {}", e) });

    trace!("output_3: {}", String::from_utf8_lossy(&output_3.stdout));

    // Unfortunately, output.stdout will be a Vec<u8> instead of a string
    // I don't really care to parse based on anything specifically, so we're
    // going to convert to a string, and then split on new lines
    let u8_buf_1 = String::from_utf8(output_1.stdout).unwrap();
    let u8_buf_2 = String::from_utf8(output_2.stdout).unwrap();
    let u8_buf_3 = String::from_utf8(output_3.stdout).unwrap();
    let stat_lines_1: Vec<&str> = u8_buf_1.split('\n').collect();
    let stat_lines_2: Vec<&str> = u8_buf_2.split('\n').collect();
    let stat_lines_3: Vec<&str> = u8_buf_3.split('\n').collect();

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
                                        .zip(stat_lines_3.iter())
    {
        let this_line_1: Vec<&str> = line_1.split_whitespace().collect();
        let this_line_2: Vec<&str> = line_2.split_whitespace().collect();
        let this_line_3: Vec<&str> = line_3.split_whitespace().collect();

        // Have we read all the cpu info yet?
        if !this_line_1[0].contains("cpu") {
            break;
        }

        let user_1 = u64::from_str(this_line_1[1]).unwrap();
        let user_2 = u64::from_str(this_line_2[1]).unwrap();
        let user_3 = u64::from_str(this_line_3[1]).unwrap();
        let nice_1 = u64::from_str(this_line_1[2]).unwrap();
        let nice_2 = u64::from_str(this_line_2[2]).unwrap();
        let nice_3 = u64::from_str(this_line_3[2]).unwrap();
        let system_1 = u64::from_str(this_line_1[3]).unwrap();
        let system_2 = u64::from_str(this_line_2[3]).unwrap();
        let system_3 = u64::from_str(this_line_3[3]).unwrap();
        let idle_1 = u64::from_str(this_line_1[4]).unwrap();
        let idle_2 = u64::from_str(this_line_2[4]).unwrap();
        let idle_3 = u64::from_str(this_line_3[4]).unwrap();

        let mut cpu_delta = Vec::<u64>::with_capacity(4);
        cpu_delta[0] = (user_2 - user_1) - (user_3 - user_2);
        cpu_delta[1] = (nice_2 - nice_1) - (nice_3 - nice_2);
        cpu_delta[2] = (system_2 - system_1) - (system_3 - system_2);
        cpu_delta[3] = (idle_2 - idle_1) - (idle_3 - idle_2);

        stat_deltas.push(cpu_delta);
    }

    // Get clock ticks per sec
    trace!("getting clk_tcks");
    let output = Command::new("getconf").arg("CLK_TCK")
        .output()
        .unwrap_or_else(|e| { panic!("failed to execute process: {}", e) });

    trace!("getconf CLK_TCK: {}", String::from_utf8_lossy(&output.stdout));

    let clk_tck = u32::from_str(
            str::from_utf8(&output.stdout).unwrap().trim())
            .unwrap();

    let mut cores = Vec::<CpuData>::with_capacity(num_cpus::get());

    let mut x = 0;
    let mut overall = 0.0f32;
    for stat_delta in stat_deltas.iter() {
        let dividend = stat_delta[0] + stat_delta[1] + stat_delta[2];
        let divisor = stat_delta[0] + stat_delta[1] + stat_delta[2] + stat_delta[3];

        let usage: f32 = (dividend as f32 / divisor as f32) * clk_tck as f32;
        if x == 0 { // Overall
            overall = usage;
        } else { // Core x
            let mut core = CpuData::new(x);
            core.set_usage(usage);
            cores.push(core);
        }
        x += 1;
    }

    Ok((overall, cores))
}

/// Returns a tuple of stats::Ram
fn get_current_ram_usage() -> Result<Ram, ()> {
    // Read from /proc/meminfo
    // For more info on stdout from command, see here:
    // https://github.com/torvalds/linux/blob/master/Documentation/filesystems/proc.txt
    let output = Command::new("cat").arg("/proc/meminfo")
        .output()
        .unwrap_or_else(|e| { panic!("failed to execute process: {}", e) });

    // Unfortunately, output.stdout will be a Vec<u8> instead of a string
    // I don't really care to parse based on anything specifically, so we're
    // going to convert to a string, and then split on new lines
    let u8_buf = String::from_utf8(output.stdout).unwrap();
    let meminfo_lines: Vec<&str> = u8_buf.split('\n').collect();

    // Current meminfo line layout
    //
    // +-----------------+-----------+-------------------+
    // |   Description   |    uint   |   uint's units    |
    // +-----------------+-----------+-------------------+
    let mut num_found = 0;
    let mut high_total = 0u64;
    let mut high_free = 0u64;
    let mut low_total = 0u64;
    let mut low_free = 0u64;
    for line in meminfo_lines.iter() {
        if line.contains("LowFree") { // Kernel remaining
            let line_split: Vec<&str> = line.split_whitespace().collect();
            low_free = u64::from_str(line_split[2]).unwrap();
            num_found += 1;
        } else if line.contains("LowTotal") { // Kernel available
            let line_split: Vec<&str> = line.split_whitespace().collect();
            low_total = u64::from_str(line_split[2]).unwrap();
            num_found += 1;
        } else if line.contains("HighFree") { // User space remaining
            let line_split: Vec<&str> = line.split_whitespace().collect();
            high_free = u64::from_str(line_split[2]).unwrap();
            num_found += 1;
        } else if line.contains("HighTotal") { // User space available
            let line_split: Vec<&str> = line.split_whitespace().collect();
            high_total = u64::from_str(line_split[2]).unwrap();
            num_found += 1;
        }

        if num_found == 4 {
            break;
        }
    }

    // Ensure we did all the wonderful string parsing correctly
    if num_found != 4 {
        error!("/proc/meminfo parsed incorrectly...?");
        return Err(());
    }

    // /proc/meminfo currently displays in kb
    let k_used = low_total - low_free;
    let k_available = low_total;
    let u_used = high_total - high_free;
    let u_available = high_total;

    Ok(Ram {
        kernel: RamData {
            bytes_used: k_used,
            bytes_available: k_available
        },
        user_space: RamData {
            bytes_used: u_used,
            bytes_available: u_available
        }
    })
}
