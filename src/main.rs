// mini_os_app_framework.rs
#![no_std]
#![no_main]
#![allow(dead_code)]

use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};

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
    use core::arch::global_asm;
    use crate::{task0_entry, task1_entry, task2_entry};
    use rtt_target::rprintln;

    pub const N_TASKS: usize = 3;
    const STACK_WORDS: usize = 256;

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct Tcb {
        pub sp: u32,
    }

    #[repr(align(8))]
    #[derive(Copy, Clone)]
    struct Stack8([u32; STACK_WORDS]);

    static mut TCBS: [Tcb; N_TASKS] = [Tcb { sp: 0 }; N_TASKS];
    static mut STACKS: [Stack8; N_TASKS] = [Stack8([0; STACK_WORDS]); N_TASKS];
    static mut CURR: usize = 0;

    const ICSR: *mut u32 = 0xE000_ED04 as *mut u32;
    const SHPR3: *mut u32 = 0xE000_ED20 as *mut u32;

    #[inline(always)]
    fn build_initial_psp(stack: &mut [u32], entry: usize) -> u32 {
        const SW: usize = 8;
        const HW: usize = 8;
        const GUARD: usize = 8;

        let len = stack.len();
        let base = len - (SW + HW + GUARD);

        for i in 0..SW {
            stack[base + i] = 0;
        }

        let hw = base + SW;
        stack[hw + 0] = 0;
        stack[hw + 1] = 0;
        stack[hw + 2] = 0;
        stack[hw + 3] = 0;
        stack[hw + 4] = 0;
        stack[hw + 5] = (task_return_trap as u32) | 1;
        stack[hw + 6] = (entry as u32) | 1;
        stack[hw + 7] = 0x0100_0000;

        let psp = unsafe { stack.as_ptr().add(base) as u32 };
        assert!(psp & 7 == 0);
        psp
    }

    extern "C" fn task_return_trap() -> ! {
        loop {
            unsafe { core::ptr::write_volatile(ICSR, 1 << 28); }
        }
    }

    pub unsafe fn init_tasks() {
        unsafe {
            let p0 = build_initial_psp(&mut STACKS[0].0, task0_entry as usize);
            let p1 = build_initial_psp(&mut STACKS[1].0, task1_entry as usize);
            let p2 = build_initial_psp(&mut STACKS[2].0, task2_entry as usize);

            TCBS[0].sp = p0;
            TCBS[1].sp = p1;
            TCBS[2].sp = p2;
            CURR = 0;

            rprintln!("p0=0x{:08X} p1=0x{:08X} p2=0x{:08X}", p0, p1, p2);
        }
    }

    pub unsafe fn init_systick_50us() {
        let mut v = unsafe { core::ptr::read_volatile(SHPR3) };
        v &= !0xFFFF_0000;
        v |= (0x80u32 << 24) | (0xFFu32 << 16);
        unsafe { core::ptr::write_volatile(SHPR3, v); }

        let syst_csr = 0xE000_E010 as *mut u32;
        let syst_rvr = 0xE000_E014 as *mut u32;
        let syst_cvr = 0xE000_E018 as *mut u32;

        unsafe { core::ptr::write_volatile(syst_rvr, 799); }
        unsafe { core::ptr::write_volatile(syst_cvr, 0); }
        unsafe { core::ptr::write_volatile(syst_csr, (1 << 2) | (1 << 1) | 1); }
    }

    pub fn start() -> ! {
        rprintln!("Starting scheduler...");
        unsafe {
            core::ptr::write_volatile(ICSR, 1 << 28);
        }
        loop {
            cortex_m::asm::wfi();
        }
    }

    global_asm!(
        r#"
        .global PendSV
        .type PendSV, %function
    PendSV:
        mrs     r0, psp
        cbz     r0, 1f
        stmdb   r0!, {{r4-r11}}
        bl      {switch}
        ldmia   r0!, {{r4-r11}}
        msr     psp, r0
        bx      lr

    1:
        bl      {switch}
        ldmia   r0!, {{r4-r11}}
        msr     psp, r0
        mrs     r1, CONTROL
        orr     r1, r1, #2
        orr     r1, r1, #1
        msr     CONTROL, r1
        isb
        ldr     lr, =0xFFFFFFFD
        bx      lr
    "#,
        switch = sym pend_sv_switch_rust
    );

    pub extern "C" fn pend_sv_switch_rust(old_psp: u32) -> u32 {
        unsafe {
            if old_psp != 0 {
                TCBS[CURR].sp = old_psp;
                CURR = (CURR + 1) % N_TASKS;
            }
            TCBS[CURR].sp
        }
    }

    #[exception]
    fn SysTick() {
        unsafe { core::ptr::write_volatile(ICSR, 1 << 28); }
    }
}


// ───────────── TASKS ─────────────

#[unsafe(no_mangle)]
pub extern "C" fn task0_entry() -> ! {
    loop {
        syscalls().gpio_toggle(GpioPin::Led1);
        for _ in 0..300 {
            cortex_m::asm::nop();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn task1_entry() -> ! {
    loop {
        syscalls().gpio_toggle(GpioPin::Led1);
        for _ in 0..800 {
            cortex_m::asm::nop();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn task2_entry() -> ! {
    static mut LAST: u64 = 0;
    loop {
        let now = syscalls().now_ms();
        unsafe {
            if now.wrapping_sub(LAST) >= 1000 {
                LAST = now;
            }
        }
        for _ in 0..1200 {
            cortex_m::asm::nop();
        }
    }
}

// ───────────── MAIN ENTRY ─────────────

static mut SYSCALLS_PTR: *mut svc::Client = core::ptr::null_mut();

#[inline(always)]
fn syscalls() -> &'static mut svc::Client {
    unsafe { &mut *SYSCALLS_PTR }
}

#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("[mini-os] Booting");

    let mut board = board::BoardSyscalls::new(
        board::RawPin::new(board::GPIOA, 5),
        board::RawPin::new(board::GPIOC, 13),
        CYCLES_PER_MS_ESTIMATE,
    );

    unsafe {
        board.init();
        svc::register_kernel_board(&mut board);
        let mut client = svc::Client::new(&mut board);
        SYSCALLS_PTR = &mut client;

        sched::init_tasks();
        sched::init_systick_50us();
    }

    sched::start();
}

////////////////////////////////////////////////////////////////////////////////////
// #![no_std]
// #![no_main]

// mod os;

// use os::os_main;

// use cortex_m_rt::entry;

// #[entry]
// fn main() -> ! {
//     os_main();
// }
