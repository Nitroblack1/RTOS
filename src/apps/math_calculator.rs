//! Math Calculator Application

use crate::app_syscalls::*;
use app_macros::app;
use rtt_target::rprintln;

#[app(id = 5, stack_size = 416, name = "math_calculator")]
pub unsafe extern "C" fn math_calculator() -> ! {
    rprintln!("[APP math_calculator] started");
    let mut operations = 0u32;
    let mut result = 1u32;

    loop {
        operations = operations.wrapping_add(1);

        // Simple math operation
        result = result.wrapping_mul(2).wrapping_add(1) % 1000;

        // Log every 3000 operations
        if operations % 3000 == 0 {
            rprintln!("[MATH] ops={}, result={}", operations, result);
        }

        yield_cpu();
    }
}