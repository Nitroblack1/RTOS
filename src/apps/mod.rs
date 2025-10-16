//! Applications Module
//!
//! This module contains all user applications in a modular structure.
//! Apps are automatically registered via #[app] macro and linker sections.

// Import all app modules (for compilation)
pub mod led_blinker;
pub mod fibonacci;
pub mod counter;
pub mod timer;
pub mod gpio_monitor;
pub mod math_calculator;
pub mod network_stack; // NEW APP ADDED!

// All app registration is now automatic via #[app] macro and linker sections!
// No more manual arrays needed.