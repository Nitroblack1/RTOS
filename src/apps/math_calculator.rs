//! Math Calculator Application
//! Computes factorial calculations iteratively using Tock-style syscalls

static mut MATH_RESULT: u32 = 1;
static mut MATH_OPERATIONS: u32 = 0;

// Automatic registration using new macro system
crate::register_app!(math_calculator_entry, 5, "math_calculator", 416);

#[unsafe(no_mangle)]
pub extern "C" fn math_calculator_entry() -> ! {
    crate::app_syscalls::debug_print(5, "Math Calculator Application started - computing factorials!");

    let mut n = 1u32;

    loop {
        unsafe {
            MATH_OPERATIONS = MATH_OPERATIONS.wrapping_add(1);

            // Calculate factorial iteratively to avoid overflow quickly
            if MATH_OPERATIONS % 800 == 0 {
                n = if n >= 10 { 1 } else { n + 1 }; // Reset at 10 to prevent overflow
                let mut factorial = 1u32;
                for i in 1..=n {
                    factorial = factorial.saturating_mul(i);
                }
                MATH_RESULT = factorial;

                if n % 3 == 0 {
                    cortex_m::interrupt::disable();
                    let _ops = core::ptr::read_volatile(core::ptr::addr_of!(MATH_OPERATIONS));
                    let _result = core::ptr::read_volatile(core::ptr::addr_of!(MATH_RESULT));
                    crate::app_syscalls::debug_print(5, "Factorial computation milestone");
                    cortex_m::interrupt::enable();
                }
            }
        }

        // Use Tock-style cooperative yielding
        crate::app_syscalls::yield_cpu();
    }
}