//! LED Blinker Application

use crate::app_syscalls::*;
use app_macros::app;
use rtt_target::rprintln;

#[app(id = 0, stack_size = 256, name = "led_blinker")]
pub unsafe extern "C" fn led_blinker() -> ! {
    rprintln!("[APP led_blinker] started");
    let mut counter = 0u32;
    loop {
        counter = counter.wrapping_add(1);
        // Only log every 1000 iterations to reduce RTT spam
        if counter % 1000 == 0 {
            rprintln!("[LED] tick {}", counter / 1000);
        }
        yield_cpu();
    }
}