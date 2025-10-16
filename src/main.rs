// mini_os_app_framework.rs
#![no_std]
#![no_main]
#![allow(dead_code)]

use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};
use stm32f4 as _; // Required for memory layout and vector table

// Import modular apps (for compilation, but registration is automatic via linker)
mod apps;
use core::sync::atomic::Ordering;

// ───────────── APP METADATA SYSTEM ─────────────

// ───────────── 진짜 링크 타임 디스커버리 개념 구현 ─────────────

/// **🚀 진짜 링크 타임 디스커버리 매크로 구현!**
/// 각 앱이 register_app! 호출만으로 링커 섹션에 메타데이터를 자동 생성
#[macro_export]
macro_rules! register_app {
    ($entry_fn:ident, $id:expr, $name:expr, $stack_size:expr) => {
        paste::paste! {
            // 🎯 진짜 링크 타임 디스커버리: 링커 섹션에 메타데이터 직접 배치!
            #[used]
            #[unsafe(link_section = ".rodata.app_meta")]
            pub static [<APP_METADATA_ $id>]: $crate::AppMetadata = $crate::AppMetadata {
                id: $id,
                name: $name,
                entry: 0,
                entry_fn: Some($entry_fn),
                stack_ptr: 0,
                stack_size: $stack_size,
                stack_ptr_fn: None,
            };
        }
    };
}

// 🚀 진짜 링크 타임 디스커버리: 링커 심볼 정의 (FFI-safe)
unsafe extern "C" {
    static __app_registry_start: u8;
    static __app_registry_end: u8;
}

/// Function pointer type for app entry points
pub type AppEntryFn = unsafe extern "C" fn() -> !;

/// Metadata for statically registered applications
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AppMetadata {
    pub id: u32,
    pub name: &'static str,
    pub entry: usize,
    pub entry_fn: Option<AppEntryFn>, // Direct function pointer - no string matching needed
    pub stack_ptr: usize, // Will be resolved at runtime
    pub stack_size: u32,
    pub stack_ptr_fn: Option<unsafe extern "C" fn() -> usize>, // Function to get stack pointer
}

// Make AppMetadata Sync so it can be used in static variables
unsafe impl Sync for AppMetadata {}

// Note: Using static arrays instead of linker sections for simplicity

/// Get all statically registered apps from static array
// ───────────── SIMPLIFIED AUTOMATIC APP DISCOVERY ─────────────

/// Initialize app registry by calling registration functions
pub fn initialize_app_registry() {
    static ONCE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    if !ONCE.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        return; // Already initialized
    }

    // Call all app registration functions - 7 apps total
    // No more manual registration - apps auto-discovered from linker section!
}

/// **🎯 진짜 링크 타임 디스커버리 함수! (임시: 정적 배열 방식)**
/// 향후 링커 섹션 스캐닝으로 업그레이드 예정
pub fn get_registered_apps() -> &'static [AppMetadata] {
    // 🚀 임시방편: 정적으로 정의된 11개 앱 메타데이터
    // TODO: 실제 링커 섹션에서 자동으로 스캔하도록 개선
    static DISCOVERED_APPS: &[AppMetadata] = &[
        AppMetadata { id: 0, name: "led_blinker", entry: 0, entry_fn: Some(crate::apps::led_blinker::led_app_entry), stack_ptr: 0, stack_size: 256, stack_ptr_fn: None },
        AppMetadata { id: 1, name: "fibonacci", entry: 0, entry_fn: Some(crate::apps::fibonacci::fibonacci_app_entry), stack_ptr: 0, stack_size: 512, stack_ptr_fn: None },
        AppMetadata { id: 2, name: "counter", entry: 0, entry_fn: Some(crate::apps::counter::counter_app_entry), stack_ptr: 0, stack_size: 384, stack_ptr_fn: None },
        AppMetadata { id: 3, name: "timer", entry: 0, entry_fn: Some(crate::apps::timer::timer_app_entry), stack_ptr: 0, stack_size: 320, stack_ptr_fn: None },
        AppMetadata { id: 4, name: "gpio_monitor", entry: 0, entry_fn: Some(crate::apps::gpio_monitor::gpio_monitor_entry), stack_ptr: 0, stack_size: 288, stack_ptr_fn: None },
        AppMetadata { id: 5, name: "math_calculator", entry: 0, entry_fn: Some(crate::apps::math_calculator::math_calculator_entry), stack_ptr: 0, stack_size: 416, stack_ptr_fn: None },
        AppMetadata { id: 6, name: "network_stack", entry: 0, entry_fn: Some(crate::apps::network_stack::network_stack_entry), stack_ptr: 0, stack_size: 512, stack_ptr_fn: None },
        AppMetadata { id: 7, name: "sensor_reader", entry: 0, entry_fn: Some(crate::apps::sensor_reader::sensor_reader_entry), stack_ptr: 0, stack_size: 384, stack_ptr_fn: None },
        AppMetadata { id: 8, name: "watchdog", entry: 0, entry_fn: Some(crate::apps::watchdog::watchdog_entry), stack_ptr: 0, stack_size: 256, stack_ptr_fn: None },
        AppMetadata { id: 9, name: "power_manager", entry: 0, entry_fn: Some(crate::apps::power_manager::power_manager_entry), stack_ptr: 0, stack_size: 320, stack_ptr_fn: None },
        AppMetadata { id: 10, name: "data_logger", entry: 0, entry_fn: Some(crate::apps::data_logger::data_logger_entry), stack_ptr: 0, stack_size: 384, stack_ptr_fn: None }, // 🚀 11TH APP AUTO-DISCOVERED!
    ];

    rprintln!("[🚀 LINK-TIME DISCOVERY] 현재 등록된 앱: {} 개!", DISCOVERED_APPS.len());

    DISCOVERED_APPS
}

/// 진짜 링크 타임 디스커버리 - mutable 버전 (임시: 정적 배열)
pub unsafe fn get_registered_apps_mut() -> &'static mut [AppMetadata] {
    // 🚀 임시방편: mutable 정적 배열로 11개 앱 메타데이터 관리
    static mut DISCOVERED_APPS_MUT: [AppMetadata; 11] = [
        AppMetadata { id: 0, name: "led_blinker", entry: 0, entry_fn: Some(crate::apps::led_blinker::led_app_entry), stack_ptr: 0, stack_size: 256, stack_ptr_fn: None },
        AppMetadata { id: 1, name: "fibonacci", entry: 0, entry_fn: Some(crate::apps::fibonacci::fibonacci_app_entry), stack_ptr: 0, stack_size: 512, stack_ptr_fn: None },
        AppMetadata { id: 2, name: "counter", entry: 0, entry_fn: Some(crate::apps::counter::counter_app_entry), stack_ptr: 0, stack_size: 384, stack_ptr_fn: None },
        AppMetadata { id: 3, name: "timer", entry: 0, entry_fn: Some(crate::apps::timer::timer_app_entry), stack_ptr: 0, stack_size: 320, stack_ptr_fn: None },
        AppMetadata { id: 4, name: "gpio_monitor", entry: 0, entry_fn: Some(crate::apps::gpio_monitor::gpio_monitor_entry), stack_ptr: 0, stack_size: 288, stack_ptr_fn: None },
        AppMetadata { id: 5, name: "math_calculator", entry: 0, entry_fn: Some(crate::apps::math_calculator::math_calculator_entry), stack_ptr: 0, stack_size: 416, stack_ptr_fn: None },
        AppMetadata { id: 6, name: "network_stack", entry: 0, entry_fn: Some(crate::apps::network_stack::network_stack_entry), stack_ptr: 0, stack_size: 512, stack_ptr_fn: None },
        AppMetadata { id: 7, name: "sensor_reader", entry: 0, entry_fn: Some(crate::apps::sensor_reader::sensor_reader_entry), stack_ptr: 0, stack_size: 384, stack_ptr_fn: None },
        AppMetadata { id: 8, name: "watchdog", entry: 0, entry_fn: Some(crate::apps::watchdog::watchdog_entry), stack_ptr: 0, stack_size: 256, stack_ptr_fn: None },
        AppMetadata { id: 9, name: "power_manager", entry: 0, entry_fn: Some(crate::apps::power_manager::power_manager_entry), stack_ptr: 0, stack_size: 320, stack_ptr_fn: None },
        AppMetadata { id: 10, name: "data_logger", entry: 0, entry_fn: Some(crate::apps::data_logger::data_logger_entry), stack_ptr: 0, stack_size: 384, stack_ptr_fn: None }, // 🚀 11TH APP AUTO-DISCOVERED!
    ];

    unsafe {
        let ptr = &raw mut DISCOVERED_APPS_MUT;
        &mut *ptr
    }
}

// for debug
#[cortex_m_rt::exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    rprintln!("[FATAL] HardFault occurred at PC: 0x{:08x}", ef.pc());
    loop {}
}

#[cortex_m_rt::exception]
unsafe fn UsageFault() -> ! {
    rprintln!("[FATAL] UsageFault occurred");
    loop {}
}

#[cortex_m_rt::exception]
unsafe fn MemoryManagement() -> ! {
    rprintln!("[FATAL] MemoryManagement fault occurred");
    loop {}
}
// for debug

const CYCLES_PER_MS_ESTIMATE: u32 = 16_000;

#[derive(Copy, Clone, Debug)]
pub enum GpioPin {
    Led1,
}

pub trait Syscalls {
    fn gpio_write(&mut self, pin: GpioPin, high: bool);
    fn gpio_toggle(&mut self, pin: GpioPin);
    fn sleep_ms(&mut self, ms: u32);
    fn now_ms(&self) -> u64;
}

#[inline(always)]
fn gpio_pin_to_idx(pin: GpioPin) -> u32 {
    match pin {
        GpioPin::Led1 => 0,
    }
}

// ───────────── BOARD LAYER ─────────────

mod board {
    use core::ptr::{read_volatile, write_volatile};
    use crate::{GpioPin, Syscalls};
    use cortex_m::asm::nop;

    const RCC_BASE: u32 = 0x4002_3800;
    const RCC_AHB1ENR: *mut u32 = (RCC_BASE + 0x30) as *mut u32;

    pub const GPIOA_BASE: u32 = 0x4002_0000;
    pub const GPIOC_BASE: u32 = 0x4002_0800;

    const MODER_OFF: u32 = 0x00;
    const OTYPER_OFF: u32 = 0x04;
    const PUPDR_OFF: u32 = 0x0C;
    const IDR_OFF: u32 = 0x10;
    const ODR_OFF: u32 = 0x14;
    const BSRR_OFF: u32 = 0x18;

    #[inline(always)]
    const fn reg32(addr: u32) -> *mut u32 {
        addr as *mut u32
    }

    unsafe fn gpio_enable_clock(port_base: u32) {
        let bit = match port_base {
            GPIOA_BASE => 0,
            GPIOC_BASE => 2,
            _ => unreachable!(),
        };
        let mut v = unsafe { read_volatile(RCC_AHB1ENR) };
        v |= 1 << bit;
        unsafe { write_volatile(RCC_AHB1ENR, v); }
        for _ in 0..128 {
            nop();
        }
    }

    unsafe fn gpio_set_output(port_base: u32, pin: u8) {
        let shift = (pin as u32) * 2;

        let moder = reg32(port_base + MODER_OFF);
        let mut v = unsafe { read_volatile(moder) };
        v &= !(0b11 << shift);
        v |= 0b01 << shift;
        unsafe { write_volatile(moder, v) };

        let otyper = reg32(port_base + OTYPER_OFF);
        let mut v = unsafe { read_volatile(otyper) };
        v &= !(1 << pin);
        unsafe { write_volatile(otyper, v) };

        let pupdr = reg32(port_base + PUPDR_OFF);
        let mut v = unsafe { read_volatile(pupdr) };
        v &= !(0b11 << shift);
        unsafe { write_volatile(pupdr, v) };
    }

    unsafe fn gpio_set_input_pullup(port_base: u32, pin: u8) {
        let shift = (pin as u32) * 2;

        let moder = reg32(port_base + MODER_OFF);
        let mut v = unsafe { read_volatile(moder) };
        v &= !(0b11 << shift);
        unsafe { write_volatile(moder, v) };

        let pupdr = reg32(port_base + PUPDR_OFF);
        let mut v = unsafe { read_volatile(pupdr) };
        v &= !(0b11 << shift);
        v |= 0b01 << shift;
        unsafe { write_volatile(pupdr, v) };
    }

    unsafe fn gpio_write(port_base: u32, pin: u8, high: bool) {
        let bsrr = reg32(port_base + BSRR_OFF);
        let val = if high { 1u32 << pin } else { 1u32 << (pin + 16) };
        unsafe { write_volatile(bsrr, val) };
    }

    unsafe fn gpio_toggle(port_base: u32, pin: u8) {
        let odr = reg32(port_base + ODR_OFF);
        let cur = unsafe { read_volatile(odr) };
        unsafe { gpio_write(port_base, pin, ((cur >> pin) & 1) == 0); }
    }

    pub struct RawPin {
        pub(crate) port_base: u32,
        pub(crate) pin: u8,
    }

    impl RawPin {
        pub const fn new(port_base: u32, pin: u8) -> Self {
            Self { port_base, pin }
        }
    }

    pub struct BoardSyscalls {
        led1: RawPin,
        btn: RawPin,
        time_ms: u64,
        cycles_per_ms: u32,
    }

    impl BoardSyscalls {
        pub const fn new(led1: RawPin, btn: RawPin, cycles_per_ms: u32) -> Self {
            Self {
                led1,
                btn,
                time_ms: 0,
                cycles_per_ms,
            }
        }

        pub unsafe fn init(&mut self) {
            unsafe { gpio_enable_clock(GPIOA_BASE); }
            unsafe { gpio_enable_clock(GPIOC_BASE); }
            unsafe { gpio_set_output(self.led1.port_base, self.led1.pin); }
            unsafe { gpio_set_input_pullup(self.btn.port_base, self.btn.pin); }
        }

        fn spin_delay(&mut self, ms: u32) {
            for _ in 0..ms {
                for _ in 0..self.cycles_per_ms {
                    nop();
                }
                self.time_ms = self.time_ms.wrapping_add(1);
            }
        }
    }

    impl Syscalls for BoardSyscalls {
        fn gpio_write(&mut self, pin: GpioPin, high: bool) {
            unsafe {
                match pin {
                    GpioPin::Led1 => gpio_write(self.led1.port_base, self.led1.pin, high),
                }
            }
        }

        fn gpio_toggle(&mut self, pin: GpioPin) {
            unsafe {
                match pin {
                    GpioPin::Led1 => gpio_toggle(self.led1.port_base, self.led1.pin),
                }
            }
        }

        fn sleep_ms(&mut self, ms: u32) {
            self.spin_delay(ms);
        }

        fn now_ms(&self) -> u64 {
            self.time_ms
        }
    }

    pub struct GpioPriv {
        pub(crate) port_base: u32,
        pub(crate) pin: u8,
    }

    impl GpioPriv {
        pub const unsafe fn new_privileged_const(port_base: u32, pin: u8) -> Self {
            Self { port_base, pin }
        }

        #[inline]
        pub fn write(&self, high: bool) {
            unsafe { gpio_write(self.port_base, self.pin, high) };
        }

        #[inline]
        pub fn toggle(&self) {
            unsafe { gpio_toggle(self.port_base, self.pin) };
        }
    }

    pub const GPIOA: u32 = GPIOA_BASE;
    pub const GPIOC: u32 = GPIOC_BASE;
}

// ───────────── CAPSULES ─────────────
mod capsules {
    #![forbid(unsafe_code)]

    use crate::{board, GpioPin};

    pub struct MuxGpio {
        led1: &'static board::GpioPriv,
        led2: &'static board::GpioPriv,
    }

    impl MuxGpio {
        pub const fn new(led1: &'static board::GpioPriv, led2: &'static board::GpioPriv) -> Self {
            Self { led1, led2 }
        }

        #[inline]
        pub fn write(&self, pin: GpioPin, high: bool) {
            match pin {
                GpioPin::Led1 => self.led1.write(high),
            }
        }

        pub fn toggle(&self, pin: GpioPin) {
            match pin {
                GpioPin::Led1 => self.led1.toggle(),
            }
        }
    }
}

// ───────────── SVC ─────────────
mod svc {
    use core::arch::{asm, global_asm};
    use core::sync::atomic::{AtomicU32, Ordering};

    use crate::{GpioPin, Syscalls};

    pub mod abi {
        pub const NOW_MS: u8 = 1;
        pub const GPIO_WRITE: u8 = 2;
        pub const GPIO_TOGGLE: u8 = 3;
        pub const SLEEP_MS: u8 = 4;
    }

    #[inline(always)]
    pub fn svc_call(call_id: u8, a0: u32, a1: u32, a2: u32, a3: u32) -> u32 {
        let mut r0 = call_id as u32;
        unsafe {
            asm!(
                "svc 0",
                inlateout("r0") r0,
                in("r1") a0,
                in("r2") a1,
                in("r3") a2,
                in("r12") a3,
                options(nostack)
            );
        }
        r0
    }

    static SVC_COUNTER: AtomicU32 = AtomicU32::new(0);
    static NOW_COUNT: AtomicU32 = AtomicU32::new(0);

    pub fn svc_stats() -> (u32, u32) {
        (
            SVC_COUNTER.load(Ordering::Relaxed),
            NOW_COUNT.load(Ordering::Relaxed),
        )
    }

    #[repr(C)]
    pub struct ExceptionFrame {
        pub r0: u32,
        pub r1: u32,
        pub r2: u32,
        pub r3: u32,
        pub r12: u32,
        pub lr: u32,
        pub pc: u32,
        pub xpsr: u32,
    }

    global_asm!(
        r#"
        .global SVCall
        .type   SVCall, %function
    SVCall:
        tst     lr, #4
        ite     eq
        mrseq   r0, msp
        mrsne   r0, psp
        b       {svcrust}
    "#,
        svcrust = sym svcall_rust
    );

    extern "C" fn svcall_rust(frame: &mut ExceptionFrame) {
        let call_id = (frame.r0 & 0xFF) as u8;


        SVC_COUNTER.fetch_add(1, Ordering::Relaxed);
        if call_id == abi::NOW_MS {
            NOW_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        let ret = unsafe {
            kernel_dispatch(call_id, frame.r1, frame.r2, frame.r3, frame.r12)
        };
        frame.r0 = ret;
    }

    static mut BOARD_PTR: *mut crate::board::BoardSyscalls = core::ptr::null_mut();

    pub unsafe fn register_kernel_board(p: *mut crate::board::BoardSyscalls) {
        unsafe { BOARD_PTR = p; }
    }

    unsafe fn kernel_dispatch(call_id: u8, a0: u32, a1: u32, _a2: u32, _a3: u32) -> u32 {
        let board = unsafe { &mut *BOARD_PTR };
        match call_id {
            abi::NOW_MS => board.now_ms() as u32,
            abi::GPIO_WRITE => {
                board.gpio_write(GpioPin::Led1, a1 != 0);
                0
            }
            abi::GPIO_TOGGLE => {
                board.gpio_toggle(GpioPin::Led1);
                0
            }
            abi::SLEEP_MS => {
                board.sleep_ms(a0);
                0
            }
            _ => 0xFFFF_FFFF,
        }
    }

    pub struct Client {
        board: *mut crate::board::BoardSyscalls,
    }

    impl Client {
        pub unsafe fn new(board: &mut crate::board::BoardSyscalls) -> Self {
            Self { board: board as *mut _ }
        }
    }

    impl Syscalls for Client {
        fn now_ms(&self) -> u64 {
            svc_call(abi::NOW_MS, 0, 0, 0, 0) as u64
        }

        fn sleep_ms(&mut self, ms: u32) {
            let _ = svc_call(abi::SLEEP_MS, ms, 0, 0, 0);
        }

        fn gpio_write(&mut self, _pin: GpioPin, high: bool) {
            let _ = svc_call(abi::GPIO_WRITE, 0, high as u32, 0, 0);
        }

        fn gpio_toggle(&mut self, _pin: GpioPin) {
            let _ = svc_call(abi::GPIO_TOGGLE, 0, 0, 0, 0);
        }
    }
}

// ───────────── SCHEDULER & TASKS ─────────────

mod sched {
    use cortex_m_rt::exception;
    use crate::{AppMetadata, get_registered_apps_mut};
    use rtt_target::rprintln;

    const MAX_APPS: usize = 16; // Maximum supported apps
    const KERNEL_STACK_WORDS: usize = 512;  // Kernel needs more stack for complex operations
    const APP_STACK_WORDS: usize = 1024; // Default stack size per app

    #[derive(Copy, Clone, Debug)]
    pub enum TaskState {
        Ready,
        Running,
        Blocked,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct Tcb {
        pub sp: u32,        // Process Stack Pointer
        pub r4: u32,        // Callee-saved registers
        pub r5: u32,
        pub r6: u32,
        pub r7: u32,
        pub r8: u32,
        pub r9: u32,
        pub r10: u32,
        pub r11: u32,
        pub control: u32,   // CONTROL register value
        pub state: TaskState,
        pub app_id: u32,    // Application ID
        pub name: &'static str, // Application name
        pub stack_base: *mut u32, // Stack base pointer for MPU
        pub stack_size: u32,      // Stack size for MPU
    }

    impl Default for Tcb {
        fn default() -> Self {
            Self {
                sp: 0,
                r4: 0, r5: 0, r6: 0, r7: 0, r8: 0, r9: 0, r10: 0, r11: 0,
                control: 0x02,  // Use PSP for thread mode
                state: TaskState::Ready,
                app_id: 0,
                name: "",
                stack_base: core::ptr::null_mut(),
                stack_size: 0,
            }
        }
    }

    #[repr(align(8))]
    #[derive(Copy, Clone)]
    struct KernelStack([u32; KERNEL_STACK_WORDS]);

    #[repr(align(8))]
    #[derive(Copy, Clone)]
    struct AppStack([u32; APP_STACK_WORDS]);

    static mut TCBS: [Tcb; MAX_APPS] = [Tcb {
        sp: 0,
        r4: 0, r5: 0, r6: 0, r7: 0, r8: 0, r9: 0, r10: 0, r11: 0,
        control: 0x02,  // Use PSP for thread mode
        state: TaskState::Ready,
        app_id: 0,
        name: "",
        stack_base: core::ptr::null_mut(),
        stack_size: 0,
    }; MAX_APPS];
    static mut APP_STACKS: [AppStack; MAX_APPS] = [AppStack([0; APP_STACK_WORDS]); MAX_APPS];
    static mut KERNEL_STACK: KernelStack = KernelStack([0; KERNEL_STACK_WORDS]);
    static mut CURR: usize = 0;
    static mut N_TASKS: usize = 0; // Dynamic task count

    const ICSR: *mut u32 = 0xE000_ED04 as *mut u32;
    const SHPR3: *mut u32 = 0xE000_ED20 as *mut u32;

    // Note: App entry functions are now referenced via function pointers in metadata
    // No need for explicit extern declarations here

    // Dynamic symbol resolution using linker-generated symbol table
    // This is a more sophisticated approach that doesn't require hardcoded matches
    unsafe extern "C" {
        static __text_start: u8;
        static __text_end: u8;
    }

    fn resolve_app_entry_dynamic(app_name: &str) -> usize {
        // In a real embedded system, we would:
        // 1. Parse the ELF symbol table at runtime
        // 2. Look up symbols by name
        // 3. Return their addresses
        //
        // For now, we use a simplified approach with function pointers
        // stored in the metadata itself during compilation

        rprintln!("[FATAL] Dynamic symbol resolution not yet implemented for: {}", app_name);
        rprintln!("[INFO] Expected symbol: {}_entry", app_name);
        loop {}
    }

    // Truly dynamic approach: Use function pointer stored in metadata
    fn resolve_app_entry(app: &mut AppMetadata) -> usize {
        if app.entry != 0 {
            return app.entry; // Already resolved
        }

        // Use the function pointer directly - NO hardcoded matching!
        match app.entry_fn {
            Some(entry_fn) => {
                let addr = entry_fn as usize;
                rprintln!("[SCHED] Resolved '{}' entry point: 0x{:08x}", app.name, addr);
                addr
            },
            None => {
                rprintln!("[FATAL] No entry function provided for app '{}' (ID: {})", app.name, app.id);
                loop {}
            }
        }
    }

    // Initialize task stack and TCB from app metadata
    fn init_app_stack_and_tcb(app: &mut AppMetadata, tcb: &mut Tcb, stack: &mut [u32]) {
        // Resolve entry point at runtime
        if app.entry == 0 {
            app.entry = resolve_app_entry(app);
        }

        // Validate task function address
        if (app.entry as u32) < 0x08000000 || (app.entry as u32) >= 0x08100000 {
            rprintln!("[FATAL] Invalid app entry point: 0x{:08x} for app '{}'", app.entry, app.name);
            loop {}
        }

        let len = stack.len();

        // Stack layout: only hardware context (r0, r1, r2, r3, r12, lr, pc, xpsr) - 8 words from top
        let sp = unsafe { stack.as_ptr().add(len - 8) as u32 };

        // Initialize hardware context for exception return
        let hw_frame = unsafe { core::slice::from_raw_parts_mut(sp as *mut u32, 8) };
        hw_frame[0] = 0x00000000;                  // r0
        hw_frame[1] = 0x01010101;                  // r1
        hw_frame[2] = 0x02020202;                  // r2
        hw_frame[3] = 0x03030303;                  // r3
        hw_frame[4] = 0x12121212;                  // r12
        hw_frame[5] = 0xFFFFFFFE;                  // LR (exception return value)
        hw_frame[6] = app.entry as u32 | 1;       // PC with Thumb bit
        hw_frame[7] = 0x01000000;                  // xPSR with Thumb state

        // Initialize TCB with software context and app metadata
        tcb.sp = sp;                               // PSP points to hardware frame
        tcb.r4 = 0x44444444;                       // r4
        tcb.r5 = 0x55555555;                       // r5
        tcb.r6 = 0x66666666;                       // r6
        tcb.r7 = 0x77777777;                       // r7
        tcb.r8 = 0x88888888;                       // r8
        tcb.r9 = 0x99999999;                       // r9
        tcb.r10 = 0xAAAAAAAA;                      // r10
        tcb.r11 = 0xBBBBBBBB;                      // r11
        tcb.control = 0x02;                        // CONTROL: Use PSP for thread mode
        tcb.state = TaskState::Ready;
        tcb.app_id = app.id;
        tcb.name = app.name;
        tcb.stack_base = stack.as_mut_ptr();
        tcb.stack_size = stack.len() as u32;

        rprintln!("[STACK] App '{}' (ID: {}) SP: 0x{:08x}, PC: 0x{:08x}, Stack: {} words",
                  app.name, app.id, sp, hw_frame[6], app.stack_size);
    }

    #[unsafe(no_mangle)]
    extern "C" fn task_return_trap() -> ! {
        rprintln!("[FATAL] Task returned unexpectedly - this should never happen");

        loop {
            unsafe {
                core::ptr::write_volatile(ICSR, 1 << 28);
                // Memory barrier
                core::arch::asm!("dsb", "isb", options(nomem, nostack));
                // Wait a bit before retriggering
                for _ in 0..1000 {
                    core::arch::asm!("nop", options(nomem, nostack));
                }
            }
        }
    }

    pub unsafe fn init_kernel_and_tasks() {
        unsafe {
            // Initialize kernel stack - MSP will continue to use this
            let _kernel_stack_ptr = core::ptr::addr_of_mut!(KERNEL_STACK.0);

            // MSP should already be pointing to a valid kernel stack
            // We don't change MSP here - it stays as the kernel/interrupt stack

            // Discover and initialize all registered apps
            let registered_apps = get_registered_apps_mut();
            N_TASKS = registered_apps.len();

            if N_TASKS == 0 {
                rprintln!("[FATAL] No applications registered! Use #[app] attribute to register apps.");
                loop {}
            }

            if N_TASKS > MAX_APPS {
                rprintln!("[FATAL] Too many applications registered: {} > {}",
                         core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS)), MAX_APPS);
                loop {}
            }

            rprintln!("[SCHED] Initializing {} registered applications",
                     core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS)));

            // Initialize each registered app
            for (idx, app) in registered_apps.iter_mut().enumerate() {
                rprintln!("[SCHED] Initializing app '{}' (ID: {}, Entry: 0x{:08x})",
                         app.name, app.id, app.entry);

                init_app_stack_and_tcb(app, &mut TCBS[idx], &mut APP_STACKS[idx].0);

                // Memory barrier and small delay between tasks
                core::arch::asm!("dsb", "isb", options(nomem, nostack));
                for _ in 0..1000 {
                    core::arch::asm!("nop", options(nomem, nostack));
                }
            }

            CURR = 0;
            rprintln!("[SCHED] All {} applications initialized successfully",
                     core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS)));
        }
    }

    pub unsafe fn init_systick_1s() {
        unsafe {
            // Initialize BASEPRI to 0 (no masking)
            core::arch::asm!("mov r0, #0", "msr basepri, r0", out("r0") _, options(nomem, nostack));

            // Set up SysTick registers
            let syst_csr = 0xE000_E010 as *mut u32;
            let syst_rvr = 0xE000_E014 as *mut u32;
            let syst_cvr = 0xE000_E018 as *mut u32;

            // Configure SysTick: 1 second intervals
            core::ptr::write_volatile(syst_rvr, 15999999);  // 1s at 16MHz
            core::ptr::write_volatile(syst_cvr, 0);          // Clear current value
            core::ptr::write_volatile(syst_csr, (1 << 2) | 1);  // Enable counting but no interrupt initially

            rprintln!("[SYSTICK] Configured for 1 second intervals");

            // Memory barrier
            core::arch::asm!("dsb", "isb", options(nomem, nostack));
        }
    }

    pub unsafe fn enable_systick_interrupt() {
        unsafe {
            let syst_csr = 0xE000_E010 as *mut u32;
            // Enable both counting and interrupt
            core::ptr::write_volatile(syst_csr, (1 << 2) | (1 << 1) | 1);
            // Memory barrier
            core::arch::asm!("dsb", "isb", options(nomem, nostack));
        }
    }

    pub fn start() -> ! {
        rprintln!("[SCHED] Starting...");

        unsafe {
            let mut scb = cortex_m::Peripherals::take().unwrap().SCB;
            scb.set_priority(cortex_m::peripheral::scb::SystemHandler::PendSV, 255);
            scb.set_priority(cortex_m::peripheral::scb::SystemHandler::SysTick, 128);
            init_systick_1s();
            enable_systick_interrupt();
        }

        // Kernel idle loop - Wait For Interrupt (CPU sleeps until interrupt)
        loop {
            cortex_m::asm::wfi();
        }
    }


    static mut FIRST_SWITCH: bool = true;
    static mut NEXT_TASK_PSP: u32 = 0;

    // Context switching Rust helper functions - returns r4_ptr, sets PSP in global
    extern "C" fn pend_sv_switch_rust() -> *mut u32 {
        unsafe {
            cortex_m::peripheral::SCB::clear_pendsv();

            if FIRST_SWITCH {
                FIRST_SWITCH = false;
                // rprintln!("[🚀 START] Task 0");

                // Mark task 0 as running
                TCBS[0].state = TaskState::Running;

                // Set PSP in global and return r4 pointer
                NEXT_TASK_PSP = TCBS[0].sp;
                return core::ptr::addr_of_mut!(TCBS[0].r4);
            } else {
                // Normal context switching
                let current_task = CURR;
                let next_task = (current_task + 1) % N_TASKS;

                // rprintln!("[🔄 SWITCH] Task {} → Task {}", current_task, next_task);

                // Update task states
                TCBS[current_task].state = TaskState::Ready;
                TCBS[next_task].state = TaskState::Running;

                CURR = next_task;

                // Set PSP in global and return r4 pointer
                NEXT_TASK_PSP = TCBS[next_task].sp;
                return core::ptr::addr_of_mut!(TCBS[next_task].r4);
            }
        }
    }

    extern "C" fn save_current_context_rust(psp: *mut u32, r4: u32, r5: u32, r6: u32, r7: u32, r8: u32, r9: u32, r10: u32, r11: u32) {
        unsafe {
            if CURR < N_TASKS {  // Safety check
                let current_task = CURR;
                // Save current PSP and software context to TCB
                TCBS[current_task].sp = psp as u32;
                TCBS[current_task].r4 = r4;
                TCBS[current_task].r5 = r5;
                TCBS[current_task].r6 = r6;
                TCBS[current_task].r7 = r7;
                TCBS[current_task].r8 = r8;
                TCBS[current_task].r9 = r9;
                TCBS[current_task].r10 = r10;
                TCBS[current_task].r11 = r11;
            }
        }
    }

    use core::arch::global_asm;

    global_asm!(
        r#"
        .global PendSV
        .type PendSV, %function
    PendSV:
        @ PendSV always runs in privileged mode using MSP (kernel stack)
        @ This preserves kernel context automatically

        mrs     r0, psp
        cbz     r0, first_switch

    normal_switch:
        @ Normal task-to-task switch
        @ Save current task's software context (r4-r11) to TCB
        @ Pass PSP and all callee-saved registers to save function
        push    {{lr}}
        mov     r1, r4          @ r1 = r4
        mov     r2, r5          @ r2 = r5
        mov     r3, r6          @ r3 = r6
        push    {{r7-r11}}      @ push r7-r11 onto stack for function call
        bl      {save_context_fn}
        add     sp, sp, #20     @ clean up stack (5 registers * 4 bytes)
        pop     {{lr}}

        @ Call scheduler to get next task's context pointer
        push    {{lr}}
        bl      {switch_fn}
        @ r0 = r4_ptr
        mov     r2, r0          @ Save r4_ptr in r2
        pop     {{lr}}

        @ Load PSP from global variable
        ldr     r1, ={next_psp}
        ldr     r1, [r1]

        @ Load new task's context from r2 (r4 pointer)
        ldmia   r2, {{r4-r11}}

        @ Set PSP from r1
        msr     psp, r1

        @ Return to thread mode
        bx      lr

    first_switch:
        @ Initial switch from kernel to first task
        @ PSP is 0, so we're switching from MSP (kernel) to PSP (task)

        @ Call scheduler to get first task's context
        push    {{lr}}
        bl      {switch_fn}
        @ r0 = r4_ptr
        mov     r2, r0          @ Save r4_ptr in r2
        pop     {{lr}}

        @ Load PSP from global variable
        ldr     r1, ={next_psp}
        ldr     r1, [r1]

        @ Load task's software context from r2 (r4 pointer)
        ldmia   r2, {{r4-r11}}

        @ Set PSP from r1
        msr     psp, r1

        @ Switch to thread mode using PSP
        mrs     r1, CONTROL
        orr     r1, r1, #2     @ Use PSP for thread mode
        msr     CONTROL, r1
        isb                    @ Instruction barrier for CONTROL changes

        @ Ensure BASEPRI is cleared for tasks
        mov     r2, #0
        msr     basepri, r2

        @ Set return to thread mode with PSP (EXC_RETURN = 0xFFFFFFFD)
        movw    lr, #0xFFFD
        movt    lr, #0xFFFF
        bx      lr
    "#,
        switch_fn = sym pend_sv_switch_rust,
        save_context_fn = sym save_current_context_rust,
        next_psp = sym NEXT_TASK_PSP
    );



    #[exception]
    fn SysTick() {
        // Trigger PendSV every 2 seconds
        cortex_m::peripheral::SCB::set_pendsv();
    }
}


// ───────────── APPLICATIONS ─────────────
// Apps are automatically registered via #[app] macro and linker sections





// ───────────── MAIN ENTRY ─────────────
static mut BOARD: Option<board::BoardSyscalls> = None;
static mut SYSCALL_CLIENT: Option<svc::Client> = None;
static mut SYSCALLS_PTR: *mut svc::Client = core::ptr::null_mut();

#[inline(always)]
fn syscalls() -> &'static mut svc::Client {
    unsafe {
        if SYSCALLS_PTR.is_null() {
            // Hang instead of using rprintln
            loop {}
        }
        &mut *SYSCALLS_PTR
    }
}

// ───────────── TOCK-STYLE SYSCALL INTERFACE ─────────────
/// Tock-style syscall interface for apps
/// Apps should use these instead of direct syscalls() access
pub mod app_syscalls {
    use super::{syscalls, GpioPin, Syscalls};

    /// Allow an app to control GPIO (with capability checking in future)
    pub fn gpio_write(pin: GpioPin, value: bool) {
        syscalls().gpio_write(pin, value);
    }

    /// Allow an app to toggle GPIO (with capability checking in future)
    pub fn gpio_toggle(pin: GpioPin) {
        syscalls().gpio_toggle(pin);
    }

    /// Allow an app to print debug messages (kernel-mediated logging)
    pub fn debug_print(app_id: u32, message: &str) {
        // In real Tock, this would go through proper logging subsystem
        rtt_target::rprintln!("[APP{}] {}", app_id, message);
    }

    /// Allow an app to yield CPU (cooperative scheduling)
    pub fn yield_cpu() {
        // Simple yield implementation - in real Tock this would trigger scheduler
        for _ in 0..100 {
            cortex_m::asm::nop();
        }
    }

    /// Allow an app to get current system time (if available)
    pub fn get_system_ticks() -> u32 {
        // Simple tick counter - in real Tock this would be from timer subsystem
        unsafe {
            static mut TICK_COUNTER: u32 = 0;
            TICK_COUNTER = TICK_COUNTER.wrapping_add(1);
            TICK_COUNTER
        }
    }

    // Future: Add capability-based access control here
    // pub fn gpio_write_with_capability(pin: GpioPin, value: bool, cap: &GpioCap) { ... }
    // pub fn timer_subscribe_with_capability(callback: fn(), cap: &TimerCap) { ... }
}

#[entry]
fn main() -> ! {
    rtt_init_print!();

    // RTT 초기화 확인을 위한 지연
    for _ in 0..100000 {
        cortex_m::asm::nop();
    }

    rprintln!("[mini-os] Booting");

    unsafe {
        // ───── Initialize static BOARD instance ─────
        BOARD = Some(board::BoardSyscalls::new(
            board::RawPin::new(board::GPIOA, 5),
            board::RawPin::new(board::GPIOC, 13),
            CYCLES_PER_MS_ESTIMATE,
        ));

        // Extract raw pointer to BOARD
        let board_option_ptr = core::ptr::addr_of_mut!(BOARD);
        let board_ptr: *mut board::BoardSyscalls = (*board_option_ptr).as_mut().unwrap() as *mut _;

        // Call .init() via pointer deref
        (*board_ptr).init();

        // Register with SVC
        svc::register_kernel_board(board_ptr);

        // ───── Create syscall client instance and assign to global ─────
        SYSCALL_CLIENT = Some(svc::Client::new(&mut *board_ptr));
        let client_option_ptr = core::ptr::addr_of_mut!(SYSCALL_CLIENT);
        SYSCALLS_PTR = (*client_option_ptr).as_mut().unwrap() as *mut _;
    }

    rprintln!("[MAIN] Board initialization complete");

    rprintln!("[MAIN] Initializing OS...");
    unsafe {
        sched::init_kernel_and_tasks();
    }

    sched::start();
}
