//! GPIO Monitor Application
//! Simulates monitoring GPIO state changes using Tock-style syscalls

static mut GPIO_MONITOR_COUNT: u32 = 0;
static mut GPIO_STATE_CHANGES: u32 = 0;

// Automatic registration using new macro system
crate::register_app!(gpio_monitor_entry, 4, "gpio_monitor", 288);

#[unsafe(no_mangle)]
pub extern "C" fn gpio_monitor_entry() -> ! {
    crate::app_syscalls::debug_print(4, "GPIO Monitor Application started - monitoring virtual GPIO!");

    loop {
        unsafe {
            GPIO_MONITOR_COUNT = GPIO_MONITOR_COUNT.wrapping_add(1);

            // Simulate GPIO state changes every 1000 iterations
            if GPIO_MONITOR_COUNT % 1000 == 0 {
                GPIO_STATE_CHANGES = GPIO_STATE_CHANGES.wrapping_add(1);

                if GPIO_STATE_CHANGES % 3 == 0 {
                    cortex_m::interrupt::disable();
                    let _changes = core::ptr::read_volatile(core::ptr::addr_of!(GPIO_STATE_CHANGES));
                    let _count = core::ptr::read_volatile(core::ptr::addr_of!(GPIO_MONITOR_COUNT));
                    crate::app_syscalls::debug_print(4, "GPIO state change detected");
                    cortex_m::interrupt::enable();
                }
            }
        }

        // Use Tock-style cooperative yielding
        crate::app_syscalls::yield_cpu();
    }
}