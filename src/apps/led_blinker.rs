//! LED Blinker Application
//! Blinks the onboard LED in a periodic pattern using Tock-style syscalls

use crate::GpioPin;
use app_macros::app;
use rtt_target::rprintln;

static mut LED_COUNTER: u32 = 0;
static mut LED_ACCUMULATOR: u32 = 0;

#[app(id = 0, stack_size = 256, name = "led_blinker")]
pub fn led_blinker() -> ! {
    // RTT 안정화 지연
    for _ in 0..10000 {
        cortex_m::asm::nop();
    }
    rprintln!("[LED] start");

    let mut counter = 0u32;

    loop {
        counter = counter.wrapping_add(1);

        if counter % 100 == 0 {
            rprintln!("[LED] {}", counter);
        }

        // CPU 양보
        for _ in 0..5000 {
            cortex_m::asm::nop();
        }
    }
}