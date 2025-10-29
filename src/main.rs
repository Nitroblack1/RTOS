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
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// ───────────── APP METADATA SYSTEM ─────────────

// ───────────── 진짜 링크 타임 디스커버리 개념 구현 ─────────────


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
    pub stack_ptr: usize,             // Will be resolved at runtime
    pub stack_size: u32,
    pub stack_ptr_fn: Option<unsafe extern "C" fn() -> usize>, // Function to get stack pointer
}

// Make AppMetadata Sync so it can be used in static variables
unsafe impl Sync for AppMetadata {}

impl AppMetadata {
    const fn empty() -> Self {
        Self {
            id: 0,
            name: "",
            entry: 0,
            entry_fn: None,
            stack_ptr: 0,
            stack_size: 0,
            stack_ptr_fn: None,
        }
    }
}

pub const MAX_APPS: usize = 16;
pub const APP_STACK_POOL_BYTES: usize = 32 * 1024;
pub const APP_STACK_POOL_WORDS: usize = APP_STACK_POOL_BYTES / 4;
pub const MIN_APP_STACK_BYTES: usize = 512;
pub const MIN_STACK_WORDS: usize = MIN_APP_STACK_BYTES / 4;
const STACK_ALIGNMENT_WORDS: usize = 2;

static APP_REGISTRY_INIT_GUARD: AtomicBool = AtomicBool::new(false);
static APP_REGISTRY_READY: AtomicBool = AtomicBool::new(false);
static APP_REGISTRY_COUNT: AtomicUsize = AtomicUsize::new(0);
static mut APP_REGISTRY: [AppMetadata; MAX_APPS] = [AppMetadata::empty(); MAX_APPS];

/// 🚀 링커 기반 앱 발견 시스템 - 실제 섹션 스캔 구현
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn discover_linker_registered_apps() -> usize {
    let start = &__app_registry_start as *const u8 as usize;
    let end = &__app_registry_end as *const u8 as usize;
    let total_bytes = end.saturating_sub(start);

    rprintln!("[REGISTRY] Scanning app registry section:");
    rprintln!("  Start: 0x{:08x}", start);
    rprintln!("  End:   0x{:08x}", end);
    rprintln!("  Size:  {} bytes", total_bytes);

    if total_bytes == 0 {
        rprintln!("[REGISTRY] No apps found in registry section");
        return 0;
    }

    let metadata_size = core::mem::size_of::<AppMetadata>();
    if total_bytes % metadata_size != 0 {
        rprintln!(
            "[FATAL] Registry section size {} is not aligned to AppMetadata size {}",
            total_bytes, metadata_size
        );
        loop {}
    }

    let app_count = total_bytes / metadata_size;
    if app_count > MAX_APPS {
        rprintln!(
            "[FATAL] Too many apps in registry: {} > {}",
            app_count, MAX_APPS
        );
        loop {}
    }

    rprintln!("[REGISTRY] Found {} registered apps", app_count);

    // Create slice from the registry section
    let app_metadata_slice = unsafe {
        core::slice::from_raw_parts(
            start as *const AppMetadata,
            app_count,
        )
    };

    // Copy apps from linker section to runtime registry
    for (i, app) in app_metadata_slice.iter().enumerate() {
        rprintln!(
            "[REGISTRY] App {}: id={} name='{}' entry_fn={:?} stack_size={}",
            i, app.id, app.name,
            app.entry_fn.map(|f| f as usize),
            app.stack_size
        );

        if i < MAX_APPS {
            unsafe { APP_REGISTRY[i] = *app; }
        }
    }

    app_count
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn populate_registry_from_linker() {
    let app_count = discover_linker_registered_apps();

    APP_REGISTRY_COUNT.store(app_count, Ordering::Release);
    rprintln!("[REGISTRY] Registry populated with {} apps", app_count);
}



/// Initialize app registry by scanning linker-provided metadata
pub fn initialize_app_registry() {
    if APP_REGISTRY_READY.load(Ordering::Acquire) {
        return;
    }

    if APP_REGISTRY_INIT_GUARD
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        unsafe {
            populate_registry_from_linker();
        }
        APP_REGISTRY_READY.store(true, Ordering::Release);
    } else {
        while !APP_REGISTRY_READY.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }
}

/// Return immutable slice of runtime-populated registry
pub fn get_registered_apps() -> &'static [AppMetadata] {
    initialize_app_registry();
    let count = APP_REGISTRY_COUNT.load(Ordering::Acquire);
    unsafe { &APP_REGISTRY[..count] }
}

/// Return mutable slice of runtime-populated registry (unsafe caller must ensure exclusivity)
#[allow(unsafe_op_in_unsafe_fn)]
pub unsafe fn get_registered_apps_mut() -> &'static mut [AppMetadata] {
    initialize_app_registry();
    let count = APP_REGISTRY_COUNT.load(Ordering::Acquire);
    &mut APP_REGISTRY[..count]
}

// for debug
#[cortex_m_rt::exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    rprintln!("[FATAL] HardFault at PC: 0x{:08x}", ef.pc());
    rprintln!("[FATAL] LR: 0x{:08x}", ef.lr());
    rprintln!("[FATAL] r0: 0x{:08x}, r1: 0x{:08x}", ef.r0(), ef.r1());
    rprintln!("[FATAL] r2: 0x{:08x}, r3: 0x{:08x}", ef.r2(), ef.r3());

    // 현재 실행 중인 태스크 정보
    rprintln!("[FATAL] Current task count: {}", sched::get_task_count());

    loop {}
}

#[cortex_m_rt::exception]
unsafe fn UsageFault() -> ! {
    rprintln!("[FATAL] UsageFault occurred");
    loop {}
}

#[cortex_m_rt::exception]
unsafe fn MemoryManagement() -> ! {
    rprintln!("[MPU] MemoryManagement fault occurred");

    // Read MPU fault status and address
    const SCB_MMFSR: *mut u8 = 0xE000_ED28 as *mut u8; // MemManage Fault Status Register
    const SCB_MMFAR: *mut u32 = 0xE000_ED34 as *mut u32; // MemManage Fault Address Register
    const SCB_CFSR: *mut u32 = 0xE000_ED28 as *mut u32; // Configurable Fault Status Register

    unsafe {
        let mmfsr = core::ptr::read_volatile(SCB_MMFSR);
        let cfsr = core::ptr::read_volatile(SCB_CFSR);
        let mmfar = if (mmfsr & 0x80) != 0 { // MMARVALID bit
            core::ptr::read_volatile(SCB_MMFAR)
        } else {
            0
        };

        rprintln!("[MPU] MMFSR: 0x{:02x}, CFSR: 0x{:08x}", mmfsr, cfsr);
        if mmfar != 0 {
            rprintln!("[MPU] Fault address: 0x{:08x}", mmfar);
        }

        // Decode fault type
        if (mmfsr & 0x01) != 0 { rprintln!("[MPU] Instruction access violation"); }
        if (mmfsr & 0x02) != 0 { rprintln!("[MPU] Data access violation"); }
        if (mmfsr & 0x08) != 0 { rprintln!("[MPU] MemManage fault on unstacking"); }
        if (mmfsr & 0x10) != 0 { rprintln!("[MPU] MemManage fault on stacking"); }
        if (mmfsr & 0x20) != 0 { rprintln!("[MPU] MemManage fault on lazy FP state preservation"); }

        // Show current task info
        rprintln!("[MPU] Current task count: {}", sched::get_task_count());

        // Clear the fault
        core::ptr::write_volatile(SCB_MMFSR, 0xFF);
    }

    rprintln!("[MPU] Task will be terminated due to memory protection violation");
    // In a real implementation, you would mark the current task as blocked/killed
    // and trigger a context switch to continue with other tasks
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
    use crate::{GpioPin, Syscalls};
    use core::ptr::{read_volatile, write_volatile};
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
        unsafe {
            write_volatile(RCC_AHB1ENR, v);
        }
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
        let val = if high {
            1u32 << pin
        } else {
            1u32 << (pin + 16)
        };
        unsafe { write_volatile(bsrr, val) };
    }

    unsafe fn gpio_toggle(port_base: u32, pin: u8) {
        let odr = reg32(port_base + ODR_OFF);
        let cur = unsafe { read_volatile(odr) };
        unsafe {
            gpio_write(port_base, pin, ((cur >> pin) & 1) == 0);
        }
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
            unsafe {
                gpio_enable_clock(GPIOA_BASE);
            }
            unsafe {
                gpio_enable_clock(GPIOC_BASE);
            }
            unsafe {
                gpio_set_output(self.led1.port_base, self.led1.pin);
            }
            unsafe {
                gpio_set_input_pullup(self.btn.port_base, self.btn.pin);
            }
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

    use crate::{GpioPin, board};

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

        let ret = unsafe { kernel_dispatch(call_id, frame.r1, frame.r2, frame.r3, frame.r12) };
        frame.r0 = ret;
    }

    static mut BOARD_PTR: *mut crate::board::BoardSyscalls = core::ptr::null_mut();

    pub unsafe fn register_kernel_board(p: *mut crate::board::BoardSyscalls) {
        unsafe {
            BOARD_PTR = p;
        }
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
            Self {
                board: board as *mut _,
            }
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

// ───────────── MPU (Memory Protection Unit) ─────────────

mod mpu {
    use rtt_target::rprintln;

    // MPU register addresses for Cortex-M4
    const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
    const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
    const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;   // Region Number Register
    const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;  // Region Base Address Register
    const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;  // Region Attribute and Size Register

    // MPU control register bits
    const MPU_CTRL_ENABLE: u32 = 1 << 0;
    const MPU_CTRL_HFNMIENA: u32 = 1 << 1;  // Enable during fault handlers
    const MPU_CTRL_PRIVDEFENA: u32 = 1 << 2; // Enable privileged default memory map

    // Region attribute bits
    const MPU_RASR_ENABLE: u32 = 1 << 0;
    const MPU_RASR_SIZE_SHIFT: u32 = 1;
    const MPU_RASR_AP_SHIFT: u32 = 24;  // Access Permission
    const MPU_RASR_XN: u32 = 1 << 28;   // Execute Never

    // Access permissions
    const MPU_AP_NO_ACCESS: u32 = 0b000;
    const MPU_AP_PRIV_RW: u32 = 0b001;      // Privileged R/W, unprivileged no access
    const MPU_AP_PRIV_RW_USER_RO: u32 = 0b010; // Privileged R/W, unprivileged R
    const MPU_AP_PRIV_RW_USER_RW: u32 = 0b011; // Privileged R/W, unprivileged R/W

    pub fn init_mpu() -> Result<(), &'static str> {
        unsafe {
            // Check if MPU is present
            let mpu_type = core::ptr::read_volatile(MPU_TYPE);
            let num_regions = (mpu_type >> 8) & 0xFF;

            if num_regions == 0 {
                rprintln!("[MPU] MPU not present on this device");
                return Err("MPU not available");
            }

            rprintln!("[MPU] Initializing MPU with {} regions", num_regions);

            // Disable MPU while configuring
            core::ptr::write_volatile(MPU_CTRL, 0);

            // Memory barrier to ensure MPU is disabled
            core::arch::asm!("dsb", "isb", options(nomem, nostack));

            // Configure basic regions
            configure_basic_regions()?;

            // Enable MPU with privileged default memory map enabled
            core::ptr::write_volatile(MPU_CTRL,
                MPU_CTRL_ENABLE | MPU_CTRL_HFNMIENA | MPU_CTRL_PRIVDEFENA);

            // Memory barrier to ensure MPU configuration is complete
            core::arch::asm!("dsb", "isb", options(nomem, nostack));

            rprintln!("[MPU] MPU initialization complete");
            Ok(())
        }
    }

    unsafe fn configure_basic_regions() -> Result<(), &'static str> {
        // STM32F446RE Memory Map:
        // Flash: 0x0800_0000 - 0x0807_FFFF (512KB)
        // SRAM:  0x2000_0000 - 0x2001_FFFF (128KB)
        // CCM:   0x1000_0000 - 0x1000_FFFF (64KB)

        unsafe {
            // Region 0: Flash memory - Privileged execute/read, no write
            configure_region(0, 0x0800_0000, region_size_encoding(512 * 1024)?,
                            MPU_AP_PRIV_RW_USER_RO, false)?; // Allow execution

            // Region 1: SRAM - Privileged access for kernel area
            // Reserve first 64KB for kernel, remaining for app stacks
            configure_region(1, 0x2000_0000, region_size_encoding(64 * 1024)?,
                            MPU_AP_PRIV_RW, true)?; // Execute never for kernel data
        }

        rprintln!("[MPU] STM32F446 memory regions configured:");
        rprintln!("  Region 0: Flash 0x0800_0000-0x0807_FFFF (512KB) - PRIV RW/USER RO");
        rprintln!("  Region 1: SRAM  0x2000_0000-0x2000_FFFF (64KB)  - PRIV RW only, XN");
        Ok(())
    }

    unsafe fn configure_region(
        region_num: u8,
        base_addr: u32,
        size_encoding: u32,
        access_permission: u32,
        execute_never: bool
    ) -> Result<(), &'static str> {
        if region_num >= 8 {
            return Err("Invalid region number");
        }

        unsafe {
            // Select region
            core::ptr::write_volatile(MPU_RNR, region_num as u32);

            // Set base address (must be aligned to region size)
            core::ptr::write_volatile(MPU_RBAR, base_addr);

            // Set region attributes
            let mut rasr = MPU_RASR_ENABLE |
                          (size_encoding << MPU_RASR_SIZE_SHIFT) |
                          (access_permission << MPU_RASR_AP_SHIFT);

            if execute_never {
                rasr |= MPU_RASR_XN;
            }

            core::ptr::write_volatile(MPU_RASR, rasr);
        }

        rprintln!("[MPU] Region {} configured: base=0x{:08x}, size={}, ap={:03b}, xn={}",
                 region_num, base_addr, size_encoding, access_permission, execute_never);

        Ok(())
    }

    // Convert size in bytes to MPU region size encoding
    // MPU region size = 2^(encoding + 1), minimum size is 32 bytes (encoding = 4)
    fn region_size_encoding(size_bytes: usize) -> Result<u32, &'static str> {
        if size_bytes < 32 {
            return Err("Region too small (minimum 32 bytes)");
        }

        // Find the power of 2 that covers the size
        let mut encoding = 4; // Start with 32 bytes (2^5 = 32)
        let mut region_size = 32;

        while region_size < size_bytes && encoding < 31 {
            encoding += 1;
            region_size <<= 1;
        }

        if encoding > 31 {
            return Err("Region too large");
        }

        Ok(encoding)
    }

    // Configure MPU for a specific task stack
    pub unsafe fn configure_task_stack_protection(
        region_num: u8,
        stack_base: *mut u32,
        stack_size: u32
    ) -> Result<(), &'static str> {
        let base_addr = stack_base as u32;
        let size_encoding = region_size_encoding(stack_size as usize)?;

        unsafe {
            configure_region(region_num, base_addr, size_encoding,
                            MPU_AP_PRIV_RW_USER_RW, true) // Stack is XN (execute never)
        }
    }

    // Test MPU protection by attempting invalid memory access
    pub unsafe fn test_mpu_protection() {
        rprintln!("[MPU] Testing memory protection...");

        // This should work - accessing current task's stack
        let test_value = 0x12345678u32;
        rprintln!("[MPU] Test 1: Valid stack access = 0x{:08x}", test_value);

        // This would trigger a fault if called from unprivileged mode:
        // Attempt to access privileged kernel area (first 64KB of SRAM)
        // let kernel_addr = 0x2000_0000 as *mut u32;
        // core::ptr::write_volatile(kernel_addr, 0xDEADBEEF);

        rprintln!("[MPU] Protection test completed");
    }
}

// ───────────── SCHEDULER & TASKS ─────────────

mod sched {
    use super::{AppMetadata, get_registered_apps_mut};
    use core::cmp;
    use cortex_m_rt::exception;
    use rtt_target::rprintln;

    const MAX_APPS: usize = super::MAX_APPS;
    const KERNEL_STACK_WORDS: usize = 512; // Kernel needs more stack for complex operations
    const APP_STACK_POOL_WORDS: usize = super::APP_STACK_POOL_WORDS;
    const MIN_STACK_WORDS: usize = super::MIN_STACK_WORDS;
    const STACK_ALIGNMENT_WORDS: usize = super::STACK_ALIGNMENT_WORDS;
    const MIN_APP_STACK_BYTES: usize = super::MIN_APP_STACK_BYTES;
    const STACK_ALIGNMENT_BYTES: usize = STACK_ALIGNMENT_WORDS * core::mem::size_of::<u32>();

    #[derive(Copy, Clone, Debug)]
    pub enum TaskState {
        Ready,
        Running,
        Blocked,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct Tcb {
        pub sp: u32, // Process Stack Pointer
        pub r4: u32, // Callee-saved registers
        pub r5: u32,
        pub r6: u32,
        pub r7: u32,
        pub r8: u32,
        pub r9: u32,
        pub r10: u32,
        pub r11: u32,
        pub control: u32, // CONTROL register value
        pub state: TaskState,
        pub app_id: u32,          // Application ID
        pub name: &'static str,   // Application name
        pub stack_base: *mut u32, // Stack base pointer for MPU
        pub stack_size: u32,      // Stack size for MPU
    }

    impl Default for Tcb {
        fn default() -> Self {
            Self {
                sp: 0,
                r4: 0,
                r5: 0,
                r6: 0,
                r7: 0,
                r8: 0,
                r9: 0,
                r10: 0,
                r11: 0,
                control: 0x02, // Use PSP for thread mode
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
    struct StackPool([u32; APP_STACK_POOL_WORDS]);

    static mut TCBS: [Tcb; MAX_APPS] = [Tcb {
        sp: 0,
        r4: 0,
        r5: 0,
        r6: 0,
        r7: 0,
        r8: 0,
        r9: 0,
        r10: 0,
        r11: 0,
        control: 0x02, // Use PSP for thread mode
        state: TaskState::Ready,
        app_id: 0,
        name: "",
        stack_base: core::ptr::null_mut(),
        stack_size: 0,
    }; MAX_APPS];
    static mut KERNEL_STACK: KernelStack = KernelStack([0; KERNEL_STACK_WORDS]);
    static mut STACK_POOL: StackPool = StackPool([0; APP_STACK_POOL_WORDS]);
    static mut STACK_POOL_OFFSET: usize = 0;
    static mut CURR: usize = 0;
    static mut N_TASKS: usize = 0; // Dynamic task count

    // 🚀 동적 스택 할당 추적 시스템
    #[derive(Copy, Clone, Debug)]
    struct StackAllocation {
        start_offset: usize,
        size_words: usize,
        task_id: u32,
        name: &'static str,
        is_free: bool,
    }

    static mut STACK_ALLOCATIONS: [StackAllocation; MAX_APPS] = [StackAllocation {
        start_offset: 0,
        size_words: 0,
        task_id: 0,
        name: "",
        is_free: true,
    }; MAX_APPS];
    static mut N_ALLOCATIONS: usize = 0;

    #[inline(always)]
    fn align_up_words(value: usize, align_words: usize) -> usize {
        (value + align_words - 1) & !(align_words - 1)
    }

    // 🚀 개선된 동적 스택 할당 시스템
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn allocate_app_stack_dynamic(words: usize, task_id: u32, name: &'static str) -> &'static mut [u32] {
        let aligned_offset = align_up_words(STACK_POOL_OFFSET, STACK_ALIGNMENT_WORDS);
        let end = aligned_offset + words;

        if end > APP_STACK_POOL_WORDS {
            rprintln!(
                "[FATAL] Stack pool exhausted: 요청 {} words, 남은 용량 {} words",
                words,
                APP_STACK_POOL_WORDS.saturating_sub(aligned_offset)
            );
            rprintln!("[STACK] Current allocations:");
            for i in 0..N_ALLOCATIONS {
                let alloc = &STACK_ALLOCATIONS[i];
                if !alloc.is_free {
                    rprintln!(
                        "  - Task '{}' (ID: {}): {} words at offset {}",
                        alloc.name, alloc.task_id, alloc.size_words, alloc.start_offset
                    );
                }
            }
            loop {}
        }

        // 할당 정보 추적
        if N_ALLOCATIONS < MAX_APPS {
            STACK_ALLOCATIONS[N_ALLOCATIONS] = StackAllocation {
                start_offset: aligned_offset,
                size_words: words,
                task_id,
                name,
                is_free: false,
            };
            N_ALLOCATIONS += 1;
        }

        STACK_POOL_OFFSET = end;
        rprintln!(
            "[STACK] Allocated {} words for task '{}' (ID: {}) at offset {}",
            words, name, task_id, aligned_offset
        );

        &mut STACK_POOL.0[aligned_offset..end]
    }

    // 🚀 스택 해제 함수 (향후 태스크 종료 시 사용)
    #[allow(unused)]
    unsafe fn deallocate_app_stack(task_id: u32) -> bool {
        unsafe {
            for i in 0..N_ALLOCATIONS {
                if STACK_ALLOCATIONS[i].task_id == task_id && !STACK_ALLOCATIONS[i].is_free {
                    STACK_ALLOCATIONS[i].is_free = true;
                    rprintln!(
                        "[STACK] Deallocated stack for task '{}' (ID: {}): {} words",
                        STACK_ALLOCATIONS[i].name,
                        task_id,
                        STACK_ALLOCATIONS[i].size_words
                    );
                    return true;
                }
            }
            false
        }
    }

    // 레거시 호환성을 위한 래퍼
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn allocate_app_stack(words: usize) -> &'static mut [u32] {
        allocate_app_stack_dynamic(words, 0, "legacy")
    }

    fn metadata_stack_bytes(app: &AppMetadata) -> usize {
        let raw = if app.stack_size == 0 {
            MIN_APP_STACK_BYTES
        } else {
            app.stack_size as usize
        };
        let aligned = if raw % STACK_ALIGNMENT_BYTES == 0 {
            raw
        } else {
            ((raw + STACK_ALIGNMENT_BYTES - 1) / STACK_ALIGNMENT_BYTES) * STACK_ALIGNMENT_BYTES
        };
        cmp::max(aligned, MIN_APP_STACK_BYTES)
    }

    fn compute_stack_words(app: &AppMetadata) -> usize {
        let bytes = metadata_stack_bytes(app);
        align_up_words((bytes + 3) / 4, STACK_ALIGNMENT_WORDS).max(MIN_STACK_WORDS)
    }

    unsafe fn acquire_app_stack(app: &mut AppMetadata) -> &'static mut [u32] {
        unsafe fn static_stack_from(
            base: usize,
            bytes: usize,
            app_name: &str,
            app_id: u32,
        ) -> &'static mut [u32] {
            if base == 0 {
                rprintln!(
                    "[FATAL] App '{}' (ID: {}) provided null stack pointer",
                    app_name,
                    app_id
                );
                loop {}
            }
            if bytes < MIN_APP_STACK_BYTES {
                rprintln!(
                    "[FATAL] App '{}' (ID: {}) stack too small: {} bytes (min {})",
                    app_name,
                    app_id,
                    bytes,
                    MIN_APP_STACK_BYTES
                );
                loop {}
            }
            if bytes % STACK_ALIGNMENT_BYTES != 0 {
                rprintln!(
                    "[FATAL] App '{}' (ID: {}) stack alignment invalid: {} bytes (alignment {})",
                    app_name,
                    app_id,
                    bytes,
                    STACK_ALIGNMENT_BYTES
                );
                loop {}
            }

            let words = bytes / core::mem::size_of::<u32>();
            unsafe { core::slice::from_raw_parts_mut(base as *mut u32, words) }
        }

        if let Some(stack_fn) = app.stack_ptr_fn {
            let base = unsafe { stack_fn() };
            let bytes = metadata_stack_bytes(app);
            unsafe { static_stack_from(base, bytes, app.name, app.id) }
        } else if app.stack_ptr != 0 {
            let bytes = metadata_stack_bytes(app);
            unsafe { static_stack_from(app.stack_ptr, bytes, app.name, app.id) }
        } else {
            let stack_words = compute_stack_words(app);
            unsafe { allocate_app_stack_dynamic(stack_words, app.id, app.name) }
        }
    }

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

        rprintln!(
            "[FATAL] Dynamic symbol resolution not yet implemented for: {}",
            app_name
        );
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
                rprintln!(
                    "[SCHED] Resolved '{}' entry point: 0x{:08x}",
                    app.name,
                    addr
                );
                addr
            }
            None => {
                rprintln!(
                    "[FATAL] No entry function provided for app '{}' (ID: {})",
                    app.name,
                    app.id
                );
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
            rprintln!(
                "[FATAL] Invalid app entry point: 0x{:08x} for app '{}'",
                app.entry,
                app.name
            );
            loop {}
        }

        if stack.len() < 8 {
            rprintln!(
                "[FATAL] Stack too small for app '{}' (ID: {}): {} words",
                app.name,
                app.id,
                stack.len()
            );
            loop {}
        }

        let stack_bytes = stack.len() * core::mem::size_of::<u32>();
        app.stack_ptr = stack.as_ptr() as usize; // Base address retained for MPU setup
        app.stack_size = stack_bytes as u32;
        let len = stack.len();

        // Stack layout: only hardware context (r0, r1, r2, r3, r12, lr, pc, xpsr) - 8 words from top
        let sp = unsafe { stack.as_ptr().add(len - 8) as u32 };

        // Initialize hardware context for exception return
        let hw_frame = unsafe { core::slice::from_raw_parts_mut(sp as *mut u32, 8) };
        hw_frame[0] = 0x00000000; // r0
        hw_frame[1] = 0x01010101; // r1
        hw_frame[2] = 0x02020202; // r2
        hw_frame[3] = 0x03030303; // r3
        hw_frame[4] = 0x12121212; // r12
        hw_frame[5] = 0xFFFFFFFE; // LR (exception return value)
        // PC validation and Thumb bit setting
        let pc_value = app.entry as u32;
        if pc_value == 0 || pc_value < 0x08000000 {
            rprintln!("[FATAL] Invalid PC for app '{}': 0x{:08x}", app.name, pc_value);
            loop {}
        }
        hw_frame[6] = pc_value | 1; // PC with Thumb bit
        hw_frame[7] = 0x01000000; // xPSR with Thumb state

        // Initialize TCB with software context and app metadata
        tcb.sp = sp; // PSP points to hardware frame
        tcb.r4 = 0x44444444; // r4
        tcb.r5 = 0x55555555; // r5
        tcb.r6 = 0x66666666; // r6
        tcb.r7 = 0x77777777; // r7
        tcb.r8 = 0x88888888; // r8
        tcb.r9 = 0x99999999; // r9
        tcb.r10 = 0xAAAAAAAA; // r10
        tcb.r11 = 0xBBBBBBBB; // r11
        tcb.control = 0x03; // CONTROL: Use PSP for thread mode + unprivileged mode
        tcb.state = TaskState::Ready;
        tcb.app_id = app.id;
        tcb.name = app.name;
        tcb.stack_base = stack.as_mut_ptr();
        tcb.stack_size = stack_bytes as u32;

        // Configure MPU protection for this task's stack
        // Use region numbers 2+ for task stacks (0,1 reserved for basic regions)
        let region_num = (app.id % 6) + 2; // Use regions 2-7 for tasks (max 6 tasks with MPU protection)
        if let Err(e) = unsafe {
            super::mpu::configure_task_stack_protection(
                region_num as u8,
                stack.as_mut_ptr(),
                stack_bytes as u32
            )
        } {
            rprintln!("[MPU] Warning: Failed to configure stack protection for app '{}': {}", app.name, e);
        } else {
            rprintln!("[MPU] Configured stack protection for app '{}' in region {}", app.name, region_num);
        }

        rprintln!(
            "[STACK] App '{}' (ID: {}) base: 0x{:08x}, SP: 0x{:08x}, PC: 0x{:08x}, Stack: {} bytes",
            app.name,
            app.id,
            app.stack_ptr,
            sp,
            hw_frame[6],
            stack_bytes
        );
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

    // 🚀 동적 태스크 스폰 함수 - 런타임에 새로운 태스크 생성!
    pub unsafe fn task_spawn(
        entry_fn: unsafe extern "C" fn() -> !,
        name: &'static str,
        stack_size_bytes: Option<usize>,
    ) -> Result<u32, &'static str> {
        unsafe {
            // TCB 슬롯 찾기
            if N_TASKS >= MAX_APPS {
                return Err("Maximum tasks reached");
            }

            // 새로운 태스크 ID 생성 (기존 태스크와 구별)
            let task_id = (N_TASKS as u32) + 1000; // 동적 태스크는 1000번대 ID 사용

            // 스택 크기 결정
            let stack_bytes = stack_size_bytes.unwrap_or(MIN_APP_STACK_BYTES);
            if stack_bytes < MIN_APP_STACK_BYTES {
                return Err("Stack size too small");
            }

            let aligned_bytes = if stack_bytes % STACK_ALIGNMENT_BYTES == 0 {
                stack_bytes
            } else {
                ((stack_bytes + STACK_ALIGNMENT_BYTES - 1) / STACK_ALIGNMENT_BYTES) * STACK_ALIGNMENT_BYTES
            };

            let stack_words = align_up_words((aligned_bytes + 3) / 4, STACK_ALIGNMENT_WORDS)
                .max(MIN_STACK_WORDS);

            // 동적 스택 할당
            let stack_slice = allocate_app_stack_dynamic(stack_words, task_id, name);

            // TCB 초기화
            let tcb_idx = N_TASKS;
            TCBS[tcb_idx] = Tcb::default();

            // 가짜 AppMetadata 생성 (동적 태스크용)
            let mut dynamic_app = AppMetadata {
                id: task_id,
                name,
                entry: entry_fn as usize,
                entry_fn: Some(entry_fn),
                stack_ptr: stack_slice.as_ptr() as usize,
                stack_size: aligned_bytes as u32,
                stack_ptr_fn: None,
            };

            // 스택과 TCB 초기화
            init_app_stack_and_tcb(&mut dynamic_app, &mut TCBS[tcb_idx], stack_slice);

            // 태스크 카운트 증가
            N_TASKS += 1;

            rprintln!(
                "[SPAWN] 🚀 Dynamically spawned task '{}' (ID: {}) with {} bytes stack at TCB index {}",
                name, task_id, aligned_bytes, tcb_idx
            );

            Ok(task_id)
        }
    }

    // 🚀 간단한 태스크 스폰 함수 (기본 스택 크기 사용)
    pub unsafe fn task_spawn_simple(
        entry_fn: unsafe extern "C" fn() -> !,
        name: &'static str,
    ) -> Result<u32, &'static str> {
        unsafe { task_spawn(entry_fn, name, None) }
    }

    // 🚀 태스크 종료 함수 (향후 구현)
    #[allow(unused)]
    pub unsafe fn task_kill(task_id: u32) -> Result<(), &'static str> {
        unsafe {
            // 동적 태스크만 종료 가능 (ID >= 1000)
            if task_id < 1000 {
                return Err("Cannot kill registered app tasks");
            }

            // TCB에서 해당 태스크 찾기
            for i in 0..N_TASKS {
                if TCBS[i].app_id == task_id {
                    // 스택 해제
                    deallocate_app_stack(task_id);

                    // TCB 초기화 (상태를 Blocked로 설정)
                    TCBS[i].state = TaskState::Blocked;
                    TCBS[i].name = "killed";

                    rprintln!("[SPAWN] 🗑️ Killed task (ID: {})", task_id);
                    return Ok(());
                }
            }

            Err("Task not found")
        }
    }

    pub unsafe fn init_kernel_and_tasks() {
        unsafe {
            // Initialize kernel stack - MSP will continue to use this
            let _kernel_stack_ptr = core::ptr::addr_of_mut!(KERNEL_STACK.0);

            // MSP should already be pointing to a valid kernel stack
            // We don't change MSP here - it stays as the kernel/interrupt stack

            // Reset stack allocator and TCB table
            STACK_POOL_OFFSET = 0;
            for idx in 0..MAX_APPS {
                TCBS[idx] = Tcb::default();
            }

            // Discover and initialize all registered apps
            let registered_apps = get_registered_apps_mut();
            N_TASKS = registered_apps.len();

            if N_TASKS == 0 {
                rprintln!("[INIT] 동적 태스크 전용 모드");
            }

            if N_TASKS > MAX_APPS {
                rprintln!(
                    "[FATAL] Too many applications registered: {} > {}",
                    core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS)),
                    MAX_APPS
                );
                loop {}
            }

            rprintln!(
                "[SCHED] Initializing {} registered applications",
                core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS))
            );

            // Initialize each registered app
            for (idx, app) in registered_apps.iter_mut().enumerate() {
                rprintln!(
                    "[SCHED] Initializing app '{}' (ID: {}, Entry: 0x{:08x})",
                    app.name,
                    app.id,
                    app.entry
                );

                let stack_slice = acquire_app_stack(app);
                TCBS[idx] = Tcb::default();
                init_app_stack_and_tcb(app, &mut TCBS[idx], stack_slice);

                // Memory barrier and small delay between tasks
                core::arch::asm!("dsb", "isb", options(nomem, nostack));
                for _ in 0..1000 {
                    core::arch::asm!("nop", options(nomem, nostack));
                }
            }

            let used_words = core::ptr::read_volatile(core::ptr::addr_of!(STACK_POOL_OFFSET));
            let used_bytes = used_words * core::mem::size_of::<u32>();
            let remaining_words = APP_STACK_POOL_WORDS.saturating_sub(used_words);
            let remaining_bytes = remaining_words * core::mem::size_of::<u32>();

            rprintln!(
                "[STACK] Pool usage: {} bytes used / {} bytes remaining ({} apps)",
                used_bytes,
                remaining_bytes,
                core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS))
            );

            CURR = 0;
            rprintln!(
                "[SCHED] All {} applications initialized successfully",
                core::ptr::read_volatile(core::ptr::addr_of!(N_TASKS))
            );
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
            core::ptr::write_volatile(syst_rvr, 15999999); // 1s at 16MHz
            core::ptr::write_volatile(syst_cvr, 0); // Clear current value
            core::ptr::write_volatile(syst_csr, (1 << 2) | 1); // Enable counting but no interrupt initially

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

        // Check if we have linker-discovered apps, if not fall back to dynamic spawning
        let task_count = unsafe { N_TASKS };
        if task_count == 0 {
            rprintln!("[SCHED] No linker-registered apps found, falling back to dynamic task spawning");
            spawn_demo_tasks();
        } else {
            rprintln!("[SCHED] Using {} linker-discovered apps", task_count);
        }

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
                rprintln!("[🚀 START] Task 0");

                // Mark task 0 as running
                TCBS[0].state = TaskState::Running;

                // Set PSP in global and return r4 pointer
                NEXT_TASK_PSP = TCBS[0].sp;
                return core::ptr::addr_of_mut!(TCBS[0].r4);
            } else {
                // Normal context switching
                let current_task = CURR;
                let next_task = (current_task + 1) % N_TASKS;

                rprintln!("[🔄 SWITCH] Task {} → Task {}", current_task, next_task);

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

    extern "C" fn save_current_context_rust(
        psp: *mut u32,
        r4: u32,
        r5: u32,
        r6: u32,
        r7: u32,
        r8: u32,
        r9: u32,
        r10: u32,
        r11: u32,
    ) {
        unsafe {
            if CURR < N_TASKS {
                // Safety check
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

        @ Switch to thread mode using PSP + unprivileged mode
        mrs     r1, CONTROL
        orr     r1, r1, #3     @ Use PSP for thread mode + unprivileged mode
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
        static mut SYSTICK_COUNT: u32 = 0;
        *SYSTICK_COUNT += 1;
        rprintln!("[SYSTICK] {} - Triggering PendSV", *SYSTICK_COUNT);
        // Trigger PendSV every 2 seconds
        cortex_m::peripheral::SCB::set_pendsv();
    }

    // 🚀 공개 API: 동적 태스크 스폰을 위한 외부 인터페이스
    pub fn spawn_dynamic_task(
        entry_fn: unsafe extern "C" fn() -> !,
        name: &'static str,
        stack_size_bytes: Option<usize>,
    ) -> Result<u32, &'static str> {
        unsafe { task_spawn(entry_fn, name, stack_size_bytes) }
    }

    pub fn spawn_simple_task(
        entry_fn: unsafe extern "C" fn() -> !,
        name: &'static str,
    ) -> Result<u32, &'static str> {
        unsafe { task_spawn_simple(entry_fn, name) }
    }

    pub fn kill_dynamic_task(task_id: u32) -> Result<(), &'static str> {
        unsafe { task_kill(task_id) }
    }

    // 🚀 스택 풀 상태 조회 함수
    pub fn get_stack_pool_usage() -> (usize, usize) {
        unsafe { (STACK_POOL_OFFSET, APP_STACK_POOL_WORDS) }
    }

    pub fn get_task_count() -> usize {
        unsafe { N_TASKS }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn demo_dynamic_worker() -> ! {
        // RTT 안정화 지연
        for _ in 0..30000 {
            cortex_m::asm::nop();
        }
        rprintln!("[DEMO] start");

        let mut count = 0u32;

        loop {
            count = count.wrapping_add(1);

            if count % 50 == 0 {
                rprintln!("[DEMO] {}", count);
            }

            // CPU 양보
            for _ in 0..5000 {
                cortex_m::asm::nop();
            }
        }
    }

    fn spawn_demo_tasks() {
        rprintln!("[SPAWN] Creating tasks...");

        // RTT 버퍼 안정화를 위한 지연
        for _ in 0..50000 {
            cortex_m::asm::nop();
        }

        // 공개된 앱 함수들을 동적 태스크로 스폰
        let apps_to_spawn = [
            (crate::apps::led_blinker::led_blinker as unsafe extern "C" fn() -> !, "led"),
            (crate::apps::fibonacci::fibonacci as unsafe extern "C" fn() -> !, "fib"),
            (crate::apps::counter::counter as unsafe extern "C" fn() -> !, "count"),
            (crate::apps::data_logger::dynamic_worker_task, "logger"),
            (demo_dynamic_worker, "demo"),
        ];

        let mut spawned_count = 0;
        for (entry_fn, name) in apps_to_spawn {
            match unsafe { task_spawn_simple(entry_fn, name) } {
                Ok(_) => {
                    rprintln!("[SPAWN] OK {}", name);
                    spawned_count += 1;
                },
                Err(_) => {
                    rprintln!("[SPAWN] FAIL {}", name);
                }
            }

            // 각 스폰 후 RTT 안정화 지연
            for _ in 0..30000 {
                cortex_m::asm::nop();
            }
        }

        rprintln!("[SPAWN] Done: {}", spawned_count);

        // 최종 로그 후 지연
        for _ in 0..50000 {
            cortex_m::asm::nop();
        }
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
    use super::{GpioPin, Syscalls, syscalls};

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

    // Initialize MPU for memory protection
    rprintln!("[MAIN] Initializing MPU...");
    match mpu::init_mpu() {
        Ok(_) => rprintln!("[MAIN] MPU initialization successful"),
        Err(e) => rprintln!("[MAIN] MPU initialization failed: {}", e),
    }

    rprintln!("[MAIN] Initializing OS...");
    unsafe {
        sched::init_kernel_and_tasks();
    }

    sched::start();
}
