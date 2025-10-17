//! Fibonacci Calculator Application
//! Computes fibonacci numbers and displays milestones using Tock-style syscalls

use app_macros::app;
use rtt_target::rprintln;

static mut APP1_COUNTER: u32 = 0;
static mut APP1_FIB_A: u32 = 0;
static mut APP1_FIB_B: u32 = 1;
static mut APP1_FIB_COUNT: u32 = 0;

#[app(id = 1, stack_size = 512, name = "fibonacci")]
pub fn fibonacci() -> ! {
    // RTT 안정화 지연
    for _ in 0..15000 {
        cortex_m::asm::nop();
    }
    rprintln!("[FIB] start");

    let mut counter = 0u32;

    loop {
        counter = counter.wrapping_add(1);

        if counter % 100 == 0 {
            rprintln!("[FIB] {}", counter);
        }

        // CPU 양보
        for _ in 0..5000 {
            cortex_m::asm::nop();
        }
    }
}