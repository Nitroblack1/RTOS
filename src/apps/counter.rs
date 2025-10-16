//! Counter Application
//! Counts numbers and computes running sum using Tock-style syscalls

static mut APP2_COUNTER: u32 = 3000;
static mut APP2_SUM: u32 = 0;

// Automatic registration using new macro system
crate::register_app!(counter_app_entry, 2, "counter", 384);

#[unsafe(no_mangle)]
pub extern "C" fn counter_app_entry() -> ! {
    crate::app_syscalls::debug_print(2, "Counter Application - prime number finder started!");

    loop {
        unsafe {
            APP2_COUNTER = APP2_COUNTER.wrapping_add(1);
            APP2_SUM = APP2_SUM.wrapping_add(APP2_COUNTER);

            // Frequent output for quick feedback
            if APP2_COUNTER % 500 == 0 {
                cortex_m::interrupt::disable();
                let _c = core::ptr::read_volatile(core::ptr::addr_of!(APP2_COUNTER));
                let _s = core::ptr::read_volatile(core::ptr::addr_of!(APP2_SUM));
                crate::app_syscalls::debug_print(2, "Counter milestone reached");
                cortex_m::interrupt::enable();
            }
        }

        // Use Tock-style cooperative yielding
        crate::app_syscalls::yield_cpu();
    }
}