//! Sensor Reader Application
//! Demonstrates true one-place app addition

use crate::app_syscalls;
use app_macros::app;

#[app(id = 7, stack_size = 384, name = "sensor_reader")]
fn sensor_reader() -> ! {
    app_syscalls::debug_print(7, "Sensor Reader app starting!");

    let mut reading_count = 0u32;

    loop {
        // Simulate sensor reading
        app_syscalls::debug_print(7, "Reading sensors...");
        reading_count = reading_count.wrapping_add(1);

        // Show sensor data
        if reading_count % 10 == 0 {
            app_syscalls::debug_print(7, "Temperature: 23°C, Humidity: 65%");
        }

        // Yield to other apps periodically
        app_syscalls::yield_cpu();

        // Simulate sensor poll interval
        for _ in 0..50000 {
            cortex_m::asm::nop();
        }
    }
}
