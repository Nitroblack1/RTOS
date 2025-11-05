//! Timer Application
//! Counts seconds and tracks elapsed time using Tock-style syscalls

use app_macros::app;

static mut TIMER_TICKS: u32 = 0;
static mut TIMER_SECONDS: u32 = 0;

#[app(id = 3, stack_size = 320, name = "timer")]
pub unsafe extern "C" fn timer() -> ! {
    // Direct RTT log to bypass syscall system
    rtt_target::rprintln!("[APP] timer ENTERED - direct RTT log");

    // Use Tock-style debug printing
    crate::app_syscalls::debug_print(3, "Timer Application started - counting seconds!");

    loop {
        unsafe {
            TIMER_TICKS = TIMER_TICKS.wrapping_add(1);

            // Approximate 1 second based on loop iterations
            if TIMER_TICKS % 2000 == 0 {
                TIMER_SECONDS = TIMER_SECONDS.wrapping_add(1);
                cortex_m::interrupt::disable();
                let _seconds = core::ptr::read_volatile(core::ptr::addr_of!(TIMER_SECONDS));
                let _ticks = core::ptr::read_volatile(core::ptr::addr_of!(TIMER_TICKS));

                // Use system ticks from kernel
                let _sys_ticks = crate::app_syscalls::get_system_ticks();

                // Use Tock-style logging (simplified - in real Tock would be more sophisticated)
                crate::app_syscalls::debug_print(3, "Timer update with system integration");

                cortex_m::interrupt::enable();
            }
        }

        // Use Tock-style cooperative yielding
        crate::app_syscalls::yield_cpu();
    }
}
