//! Network Stack Application
//! Simulates network packet processing using Tock-style syscalls

static mut PACKET_COUNT: u32 = 0;
static mut BYTES_PROCESSED: u32 = 0;

// Automatic registration - just add this line!
crate::register_app!(network_stack_entry, 6, "network_stack", 512);

#[unsafe(no_mangle)]
pub extern "C" fn network_stack_entry() -> ! {
    crate::app_syscalls::debug_print(6, "Network Stack Application started - processing packets!");

    loop {
        unsafe {
            PACKET_COUNT = PACKET_COUNT.wrapping_add(1);
            BYTES_PROCESSED = BYTES_PROCESSED.wrapping_add(64); // Simulate 64-byte packets

            // Report every 1000 packets
            if PACKET_COUNT % 1000 == 0 {
                cortex_m::interrupt::disable();
                let _packets = core::ptr::read_volatile(core::ptr::addr_of!(PACKET_COUNT));
                let _bytes = core::ptr::read_volatile(core::ptr::addr_of!(BYTES_PROCESSED));
                crate::app_syscalls::debug_print(6, "Network packet milestone reached");
                cortex_m::interrupt::enable();
            }
        }

        // Use Tock-style cooperative yielding
        crate::app_syscalls::yield_cpu();
    }
}