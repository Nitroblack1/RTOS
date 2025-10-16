//! LED Blinker Application
//! Blinks the onboard LED in a periodic pattern using Tock-style syscalls

use rtt_target::rprintln;
use crate::GpioPin;

static mut LED_COUNTER: u32 = 0;
static mut LED_ACCUMULATOR: u32 = 0;

// Automatic Tock-style registration via linker section
crate::register_app!(led_app_entry, 0, "led_blinker", 256);

#[unsafe(no_mangle)]
pub extern "C" fn led_app_entry() -> ! {
    rprintln!("[APP0] LED Blinker - Tock-style syscall interface!");

    // Turn on LED using Tock-style app syscalls
    crate::app_syscalls::gpio_write(GpioPin::Led1, true);

    loop {
        unsafe {
            LED_COUNTER = LED_COUNTER.wrapping_add(1);
            LED_ACCUMULATOR = LED_ACCUMULATOR.wrapping_add(LED_COUNTER * 7);

            // Show progress with LED patterns (fast feedback)
            if LED_COUNTER % 5000 == 0 {
                // LED toggle using Tock-style syscall interface
                crate::app_syscalls::gpio_toggle(GpioPin::Led1);
            }

            // Frequent output for quick feedback
            if LED_COUNTER % 500 == 0 {
                cortex_m::interrupt::disable();
                let c = core::ptr::read_volatile(core::ptr::addr_of!(LED_COUNTER));
                let a = core::ptr::read_volatile(core::ptr::addr_of!(LED_ACCUMULATOR));
                rprintln!("[DEBUG] App0 - Counter: {}, Accum: 0x{:08x}", c, a);
                cortex_m::interrupt::enable();
            }
        }

        // Simple delay
        for _ in 0..1000 {
            cortex_m::asm::nop();
        }
    }
}