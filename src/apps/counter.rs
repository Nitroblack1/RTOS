//! Counter Application
//! Counts numbers and computes running sum using Tock-style syscalls

use app_macros::app;
use rtt_target::rprintln;

static mut APP2_COUNTER: u32 = 3000;
static mut APP2_SUM: u32 = 0;

#[app(id = 2, stack_size = 384, name = "counter")]
pub fn counter() -> ! {
    // RTT 안정화 지연
    for _ in 0..20000 {
        cortex_m::asm::nop();
    }
    rprintln!("[COUNT] start");

    let mut counter = 0u32;

    loop {
        counter = counter.wrapping_add(1);

        if counter % 100 == 0 {
            rprintln!("[COUNT] {}", counter);
        }

        // CPU 양보
        for _ in 0..5000 {
            cortex_m::asm::nop();
        }
    }
}