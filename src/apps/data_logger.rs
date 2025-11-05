//! Data Logger Application

use crate::app_syscalls::*;
use app_macros::app;
use rtt_target::rprintln;

#[app(id = 10, stack_size = 384, name = "data_logger")]
pub unsafe extern "C" fn data_logger() -> ! {
    rprintln!("[APP data_logger] started");
    let mut data_points = 0u32;

    loop {
        data_points = data_points.wrapping_add(1);

        // Simulate data logging - only log every 4000 data points
        if data_points % 4000 == 0 {
            rprintln!("[DATA] logged {} data points", data_points);
        }

        yield_cpu();
    }
}