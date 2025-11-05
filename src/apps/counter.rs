//! Counter Application

use crate::app_syscalls::*;
use app_macros::app;
use rtt_target::rprintln;

#[app(id = 2, stack_size = 384, name = "counter")]
pub unsafe extern "C" fn counter() -> ! {
    rprintln!("[APP counter] started");
    let mut count = 0u32;
    loop {
        count = count.wrapping_add(1);
        // Only log every 800 iterations for testing
        if count % 800 == 0 {
            rprintln!("[COUNT] {}", count);
        }
        yield_cpu();
    }
}