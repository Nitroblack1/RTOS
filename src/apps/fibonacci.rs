//! Fibonacci Calculator Application
//! Computes fibonacci numbers and displays milestones using Tock-style syscalls

static mut APP1_COUNTER: u32 = 0;
static mut APP1_FIB_A: u32 = 0;
static mut APP1_FIB_B: u32 = 1;
static mut APP1_FIB_COUNT: u32 = 0;

// Automatic registration using new macro system
crate::register_app!(fibonacci_app_entry, 1, "fibonacci", 512);

#[unsafe(no_mangle)]
pub extern "C" fn fibonacci_app_entry() -> ! {
    crate::app_syscalls::debug_print(1, "Fibonacci Application started - computing fibonacci sequence!");

    loop {
        unsafe {
            APP1_COUNTER = APP1_COUNTER.wrapping_add(1);

            // Calculate fibonacci every 100 iterations
            if APP1_COUNTER % 100 == 0 {
                let fib_next = APP1_FIB_A.wrapping_add(APP1_FIB_B);
                APP1_FIB_A = APP1_FIB_B;
                APP1_FIB_B = fib_next;
                APP1_FIB_COUNT = APP1_FIB_COUNT.wrapping_add(1);

                // Quick fibonacci milestones (every 5 calculations)
                if APP1_FIB_COUNT % 5 == 0 {
                    cortex_m::interrupt::disable();
                    let _count = core::ptr::read_volatile(core::ptr::addr_of!(APP1_FIB_COUNT));
                    let _fib = core::ptr::read_volatile(core::ptr::addr_of!(APP1_FIB_B));
                    crate::app_syscalls::debug_print(1, "Fibonacci milestone reached");
                    cortex_m::interrupt::enable();
                }
            }
        }

        // Use Tock-style cooperative yielding
        crate::app_syscalls::yield_cpu();
    }
}