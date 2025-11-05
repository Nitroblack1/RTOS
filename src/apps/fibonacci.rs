//! Fibonacci Calculator Application
//! Computes fibonacci numbers and displays milestones using Tock-style syscalls

use app_macros::app;
use rtt_target::rprintln;

static mut APP1_COUNTER: u32 = 0;
static mut APP1_FIB_A: u32 = 0;
static mut APP1_FIB_B: u32 = 1;
static mut APP1_FIB_COUNT: u32 = 0;

#[app(id = 1, stack_size = 512, name = "fibonacci")]
pub unsafe extern "C" fn fibonacci() -> ! {
    rprintln!("[APP fibonacci] started");

    use crate::app_syscalls::yield_cpu;
    let mut fib_a = 0u32;
    let mut fib_b = 1u32;
    let mut iterations = 0u32;

    loop {
        iterations = iterations.wrapping_add(1);

        // Compute next fibonacci number
        let next = fib_a.wrapping_add(fib_b);
        fib_a = fib_b;
        fib_b = next;

        // Only log every 1000 iterations for testing
        if iterations % 1000 == 0 {
            rprintln!("[FIB] iter {}, fib={}", iterations, fib_b);
        }

        yield_cpu();
    }
}