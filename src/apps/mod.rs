//! Applications Module
//!
//! This module contains all user applications in a modular structure.
//! Apps are automatically registered via the `#[app]` attribute macro.

// Import all app modules for compilation - apps are automatically discovered by the build system
pub mod counter;
pub mod data_logger;
pub mod fibonacci;
pub mod gpio_monitor;
pub mod led_blinker;
pub mod math_calculator;
pub mod network_stack;
pub mod power_manager; // 🚀 10번째 앱 - 진짜 링크 타임 디스커버리 검증!
pub mod sensor_reader; // NEW APP ADDED AUTOMATICALLY!
pub mod timer;
pub mod watchdog; // 링크 타임 디스커버리로 자동 발견! // 🚀 11번째 앱 - 최종 자동 등록 검증!

// No manual registration arrays needed - apps automatically discovered!
