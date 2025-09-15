// mini_os_app_framework.rs
// - Board: STM32F446 (e.g., Nucleo-F446RE)
#![no_std]
#![no_main]
#![allow(dead_code)]


use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};


// ------------------------- Capsules (no unsafe) ------------------------
// In Tock-style, capsules virtualize/mediate privileged access.
// So, no unsafe code here so that only the core kernel (board) can use it
mod capsules {
    #![forbid(unsafe_code)]
    use crate::{board, os::GpioPin};
    
    // MuxGpio: 여러 클라이언트가 하나의 GPIO 핀을 공유할 수 있게 함
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
                GpioPin::Led2 => self.led2.write(high),
            }
        }
        pub fn toggle(&self, pin: GpioPin) {
            match pin {
                GpioPin::Led1 => self.led1.toggle(),
                GpioPin::Led2 => self.led2.toggle(),
            }
        }
    }
}

// ------------------------- SVC layer ------------------------
mod svc {
    // --------------------- for rtt debug ----------------------
    use core::sync::atomic::{ AtomicU32, Ordering };

    static SVC_COUNTER: AtomicU32 = AtomicU32::new(0);
    static NOW_COUNT: AtomicU32 = AtomicU32::new(0);

    pub fn svc_stats() -> (u32, u32) {
        (
            SVC_COUNTER.load(Ordering::Relaxed),
            NOW_COUNT.load(Ordering::Relaxed)
        )
    }
    // --------------------- end rtt debug ----------------------

    use core::arch::{ asm, global_asm };
    use crate::os::Syscalls;

    // --------- ABI : call_id definitions ----------
    pub mod abi {
        pub const NOW_MS: u8 = 1;
        pub const GPIO_WRITE: u8 = 2;
        pub const GPIO_TOGGLE: u8 = 3;
        pub const SLEEP_MS: u8 = 4;
    }

    // --------- 공용 SVC call wrapper ----------------
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

    // --------- (1) ExceptionFrame 구조체 정의 ----------
    // 참고: https://interrupt.memfault.com/blog/cortex-m-exception-handling
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

    // --------- (2) SVC 핸들러 어셈블리: ExceptionFrame 포인터를 인수로 전달 ----------
    global_asm!(
        r#"
        .global SVCall
        .type   SVCall, %function
    SVCall:
        tst     lr, #4      // EXC_RETURN bit 2: 0=MSP, 1=PSP
        ite     eq          // if-then-else
        mrseq   r0, msp    // r0 = stack ptr (MSP or PSP)
        mrsne   r0, psp    // (on thread mode)
        b       {svcrust}  // call Rust handler
    "#,
        svcrust = sym crate::svc::svcall_rust
    );

    // --------- (3) SVC 핸들러: ExceptionFrame 포인터 인수로 받음 ----------
    extern "C" fn svcall_rust(frame: &mut ExceptionFrame) {
        let call_id = (frame.r0 & 0xFF) as u8;

        // ------------------ for rtt debug -------------------
        SVC_COUNTER.fetch_add(1, Ordering::Relaxed);
        match call_id {
            abi::NOW_MS => { NOW_COUNT.fetch_add(1, Ordering::Relaxed); }
            _ => {}
        }
        // ---------------- end rtt debug ---------------------

        let a0 = frame.r1;
        let a1 = frame.r2;
        let a2 = frame.r3;
        let a3 = frame.r12;

        let ret = unsafe { kernel_dispatch(call_id, a0, a1, a2, a3) };
        frame.r0 = ret;
    }

    // --------- (4) 커널(Board) 접근 포인터 등록 ----------
    static mut BOARD_PTR: *mut crate::board::BoardSyscalls = core::ptr::null_mut();
    pub unsafe fn register_kernel_board(p: *mut crate::board::BoardSyscalls) {
        unsafe { BOARD_PTR = p };
    }

    // --------- (5) 실제 디스패처 ----------
    unsafe fn kernel_dispatch(call_id: u8, a0: u32, a1: u32, _a2: u32, _a3: u32) -> u32 {
        let board = unsafe { &mut *BOARD_PTR };
        match call_id {
            abi::NOW_MS => board.now_ms() as u32,

            abi::GPIO_WRITE => {
                let pin_enum = if a0 == 0 { crate::os::GpioPin::Led1 } 
                                        else { crate::os::GpioPin::Led2 };
                board.gpio_write(pin_enum, a1 != 0);
                0
            }

            abi::GPIO_TOGGLE => {
                let pin_enum = if a0 == 0 { crate::os::GpioPin::Led1 } else { crate::os::GpioPin::Led2 };
                board.gpio_toggle(pin_enum);
                0
            }

            abi::SLEEP_MS => {
                board.sleep_ms(a0);
                0
            }

            _ => 0xFFFF_FFFF, // unknown
        }
    }

    // --------- (6) Syscalls용 SVC 클라이언트 래퍼 ----------
    // fallback을 위해 보드 포인터 보관 (raw pointer로 보관: 빌림 충돌 회피)
    pub struct Client {
        board: *mut crate::board::BoardSyscalls
    }
    impl Client {
        pub unsafe fn new(board: &mut crate::board::BoardSyscalls) -> Self {
            Self { board: board as *mut _ }
        }
    }

    // 기존 os::Syscalls 트레이트를 이 클라이언트가 구현
    impl crate::os::Syscalls for Client {
        fn now_ms(&self) -> u64 {
            svc_call(abi::NOW_MS, 0, 0, 0, 0) as u64
        }
        fn sleep_ms(&mut self, ms: u32) {
            let _ = svc_call(abi::SLEEP_MS, ms, 0, 0, 0);
        }
        fn gpio_write(&mut self, pin: crate::os::GpioPin, high: bool) {
            let p = match pin {
                crate::os::GpioPin::Led1 => 0u32,
                crate::os::GpioPin::Led2 => 1u32,
            };
            let h = if high { 1u32 } else { 0u32 };
            let _ = svc_call(abi::GPIO_WRITE, p, h, 0, 0);
        }
        fn gpio_toggle(&mut self, pin: crate::os::GpioPin) {
            let p = match pin {
                crate::os::GpioPin::Led1 => 0u32,
                crate::os::GpioPin::Led2 => 1u32,
            };
            let _ = svc_call(abi::GPIO_TOGGLE, p, 0, 0, 0);
        }
    }
}

// ------------------------- OS Core -------------------------
mod os {
    #[derive(Copy, Clone, Debug)]
    pub enum GpioPin { Led1, Led2 }

    pub trait Syscalls {
        fn gpio_write(&mut self, pin: GpioPin, high: bool);
        fn gpio_toggle(&mut self, pin: GpioPin);
        fn sleep_ms(&mut self, ms: u32);
        fn now_ms(&self) -> u64;
    }
}

// -------------- Board layer: STM32F446 raw registers ---------
mod board {
    use core::ptr::{read_volatile, write_volatile};
    use super::os::{GpioPin, Syscalls};
    use cortex_m::asm::nop;

    // --- RCC base (STM32F4xx) ---
    const RCC_BASE: u32 = 0x4002_3800;
    const RCC_AHB1ENR: *mut u32 = (RCC_BASE + 0x30) as *mut u32; // GPIOxEN bits

    // --- GPIO base ---
    pub const GPIOA_BASE: u32 = 0x4002_0000;
    pub const GPIOC_BASE: u32 = 0x4002_0800;

    // Offsets (only what we use)
    const MODER_OFF:  u32 = 0x00;
    const OTYPER_OFF: u32 = 0x04;
    const PUPDR_OFF:  u32 = 0x0C;
    const IDR_OFF:    u32 = 0x10;
    const ODR_OFF:    u32 = 0x14;
    const BSRR_OFF:   u32 = 0x18;

    #[inline(always)]
    const fn reg32(addr: u32) -> *mut u32 { addr as *mut u32 }

    unsafe fn gpio_enable_clock(port_base: u32) {
        // AHB1ENR: bit0=GPIOA, bit2=GPIOC
        let bit = match port_base { GPIOA_BASE => 0, GPIOC_BASE => 2, _ => unreachable!() };
        let mut v = unsafe { read_volatile(RCC_AHB1ENR) };
        v |= 1 << bit;
        unsafe { write_volatile(RCC_AHB1ENR, v) };
        for _ in 0..128 { nop(); }
    }

    unsafe fn gpio_set_output(port_base: u32, pin: u8) {
        // MODER: 01 = output
        let moder = reg32(port_base + MODER_OFF);
        let mut v = unsafe { read_volatile(moder) };
        let shift = (pin as u32) * 2;
        v &= !(0b11 << shift);
        v |=  0b01 << shift;
        unsafe { write_volatile(moder, v) };

        // OTYPER: push-pull
        let otyper = reg32(port_base + OTYPER_OFF);
        let mut v = unsafe { read_volatile(otyper) };
        v &= !(1 << pin);
        unsafe { write_volatile(otyper, v) };

        // PUPDR: no pull
        let pupdr = reg32(port_base + PUPDR_OFF);
        let mut v = unsafe { read_volatile(pupdr) };
        let shift2 = (pin as u32) * 2;
        v &= !(0b11 << shift2);
        unsafe { write_volatile(pupdr, v) };
    }

    unsafe fn gpio_set_input_pullup(port_base: u32, pin: u8) {
        // MODER: 00 = input
        let moder = reg32(port_base + MODER_OFF);
        let mut v = unsafe { read_volatile(moder) };
        let shift = (pin as u32) * 2;
        v &= !(0b11 << shift);
        unsafe { write_volatile(moder, v) };

        // PUPDR: 01 = pull-up
        let pupdr = reg32(port_base + PUPDR_OFF);
        let mut v = unsafe { read_volatile(pupdr) };
        v &= !(0b11 << shift);
        v |=  0b01 << shift;
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
        let high = ((cur >> pin) & 1) == 0;
        unsafe { gpio_write(port_base, pin, high) };
    }

    unsafe fn gpio_read_input(port_base: u32, pin: u8) -> bool {
        let idr = reg32(port_base + IDR_OFF);
        let v = unsafe { read_volatile(idr) };
        ((v >> pin) & 1) != 0
    }

    pub struct RawPin { pub(crate) port_base: u32, pub(crate) pin: u8 }
    impl RawPin { pub const fn new(port_base: u32, pin: u8) -> Self { Self { port_base, pin } } }

    pub struct BoardSyscalls {
        led1: RawPin, // PA5
        led2: RawPin, // PA5
        btn:  RawPin, // PC13
        time_ms: u64,
        cycles_per_ms: u32,
    }

    impl BoardSyscalls {
        pub const fn new(led1: RawPin, led2: RawPin, btn: RawPin, cycles_per_ms: u32) -> Self {
            Self { led1, led2, btn, time_ms: 0, cycles_per_ms }
        }

        pub unsafe fn init(&mut self) {
            unsafe {
                gpio_enable_clock(GPIOA_BASE);
                gpio_enable_clock(GPIOC_BASE);
                gpio_set_output(self.led1.port_base, self.led1.pin);
                gpio_set_output(self.led2.port_base, self.led2.pin);
                gpio_set_input_pullup(self.btn.port_base, self.btn.pin);
            }
        }

        fn spin_delay(&mut self, ms: u32) {
            for _ in 0..ms {
                for _ in 0..self.cycles_per_ms { nop(); }
                self.time_ms = self.time_ms.wrapping_add(1);
            }
        }
    }

    impl Syscalls for BoardSyscalls {
        fn gpio_write(&mut self, pin: GpioPin, high: bool) {
            unsafe {
                match pin {
                    GpioPin::Led1 => gpio_write(self.led1.port_base, self.led1.pin, high),
                    GpioPin::Led2 => gpio_write(self.led2.port_base, self.led2.pin, high),
                }
            }
        }
        fn gpio_toggle(&mut self, pin: GpioPin) {
            unsafe {
                match pin {
                    GpioPin::Led1 => gpio_toggle(self.led1.port_base, self.led1.pin),
                    GpioPin::Led2 => gpio_toggle(self.led2.port_base, self.led2.pin),
                }
            }
        }
        fn sleep_ms(&mut self, ms: u32) { self.spin_delay(ms); }
        fn now_ms(&self) -> u64 { self.time_ms }
    }

    // ------------- Privileged Object: GpioPriv -----------------
    pub struct GpioPriv { pub(crate) port_base: u32, pub(crate) pin: u8 }
    impl GpioPriv {
        pub const unsafe fn new_privileged_const(port_base: u32, pin: u8) -> Self {
            Self { port_base, pin }
        }
        #[inline]
        pub fn write(&self, high: bool) {
            unsafe { gpio_write(self.port_base, self.pin, high); }
        }
        #[inline]
        pub fn toggle(&self) {
            unsafe { gpio_toggle(self.port_base, self.pin); }
        }
    }
    
    pub const GPIOA: u32 = GPIOA_BASE;
    pub const GPIOC: u32 = GPIOC_BASE;
}



// ------------------ Privileged objects & capsules ------------------
// Core (kernel) creates privileged objects with const-unsafe constructors.
// Capsules hold only safe references and expose safe methods to SVC.
static GPIO_LED1_PRIV: board::GpioPriv = unsafe { board::GpioPriv::new_privileged_const(board::GPIOA, 5) };
static GPIO_LED2_PRIV: board::GpioPriv = unsafe { board::GpioPriv::new_privileged_const(board::GPIOA, 5) };
pub(crate) static GPIO_CAP: capsules::MuxGpio = capsules::MuxGpio::new(&GPIO_LED1_PRIV, &GPIO_LED2_PRIV);

// --------------------------- Schedular (preemptive RR @ 50us) ---------------------------
mod sched {
    use core::arch::global_asm;
    use cortex_m_rt::exception;
    use rtt_target::{rprintln};
    use crate::{task0_entry, task1_entry, task2_entry};

    pub const N_TASKS: usize = 3;
    const STACK_WORDS: usize = 256; // 1KB stack per task

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct Tcb {
        pub sp: u32,    // Process SP for this task
    }

    // Task table (PSP contexts) and stacks
    pub static mut TCBS: [Tcb; N_TASKS] = [Tcb { sp: 0 }; N_TASKS];
    
    #[repr(align(8))]
    #[derive(Copy, Clone)]
    struct Stack8([u32; STACK_WORDS]);
    static mut STACKS: [Stack8; N_TASKS] = [Stack8([0; STACK_WORDS]); N_TASKS];
    pub static mut CURR: usize = 0;

     // Addresses for SCB registers (for priority+PendSV trigger)
    const ICSR: *mut u32  = 0xE000_ED04 as *mut u32; // bit28 = PENDSVSET
    const SHPR3:*mut u32  = 0xE000_ED20 as *mut u32; // [31:24]=SysTick, [23:16]=PendSV

    #[inline(always)]
    fn build_initial_psp(stack: &mut [u32], entry_addr: u32) -> u32 {
        let total_words = 8 + 8; let len = stack.len(); let base = len - total_words;
        for i in 0..8 { stack[base + i] = 0; } // r4-r11
        // 하드웨어 프레임 (R0..R3, R12, LR, PC, xPSR)
        stack[base + 8 + 0] = 0;                        // R0
        stack[base + 8 + 1] = 0;                        // R1
        stack[base + 8 + 2] = 0;                        // R2
        stack[base + 8 + 3] = 0;                        // R3
        stack[base + 8 + 4] = 0;                        // R12
        stack[base + 8 + 5] = task_return_trap as u32;  // LR
        stack[base + 8 + 6] = entry_addr as u32;        // PC
        stack[base + 8 + 7] = 0x0100_0000;              // xPSR (T-bit)
        (stack.as_ptr().wrapping_add(base)) as usize as u32
    }

    extern "C" fn task_return_trap() -> ! {
        loop {
            unsafe {
                core::ptr::write_volatile(ICSR, 1 << 28);
            }
            cortex_m::asm::bkpt();
        }
    }

    pub unsafe fn init_systick_50us() {
        let mut v = unsafe { core::ptr::read_volatile(SHPR3) };
        v &= !0xFFFF_0000;
        v |= (0x80u32 << 24) | (0xFFu32 << 16);
        unsafe { core::ptr::write_volatile(SHPR3, v) };

        let syst_csr  = 0xE000_E010 as *mut u32;
        let syst_rvr  = 0xE000_E014 as *mut u32;
        let syst_cvr  = 0xE000_E018 as *mut u32;

        unsafe { core::ptr::write_volatile(syst_rvr, 799) };
        unsafe { core::ptr::write_volatile(syst_cvr, 0) };
        unsafe { core::ptr::write_volatile(syst_csr, (1<<2) | (1<<1) | 1) };
    }

    pub unsafe fn init_tasks() {
        let (p0, p1, p2);

        unsafe {
            p0 = build_initial_psp(&mut STACKS[0].0, (task0_entry as usize) as u32);
            p1 = build_initial_psp(&mut STACKS[1].0, (task1_entry as usize) as u32);
            p2 = build_initial_psp(&mut STACKS[2].0, (task2_entry as usize) as u32);
            TCBS[0].sp = p0; TCBS[1].sp = p1; TCBS[2].sp = p2; CURR = 0;
        }
    }

    pub fn start() -> ! {
        rprintln!("Starting tasks... ");
        unsafe {
            core::ptr::write_volatile(ICSR, 1 << 28);
        }
        loop { cortex_m::asm::wfi(); }
    }

    global_asm!(
        r#"
        .global PendSV
        .type   PendSV, %function
    PendSV:
        mrs     r0, psp
        stmdb   r0!, {{r4-r11}}
        bl      {switch}
        ldmia   r0!, {{r4-r11}}
        msr     psp, r0

        mrs     r1, CONTROL
        orr     r1, r1, #2      // SPSEL=1(PSP)
        orr     r1, r1, #1      // nPRIV=1(Unpriviledged)
        msr     CONTROL, r1
        isb

        bx      lr
    "#

    , switch = sym crate::sched::pend_sv_switch_rust
    );

    pub extern "C" fn pend_sv_switch_rust(old_psp: u32) -> u32 {
        unsafe { TCBS[CURR].sp = old_psp; CURR = (CURR + 1) % N_TASKS; TCBS[CURR].sp }
    }

    #[exception]
    fn SysTick() { unsafe { core::ptr::write_volatile(ICSR, 1 << 28); } }
}


use crate::os::Syscalls;
static mut SYSCALLS_PTR: *mut svc::Client = core::ptr::null_mut();
#[inline(always)]
fn syscalls() -> &'static mut svc::Client {
    unsafe { &mut *SYSCALLS_PTR }
}

// --------------------------- Tasks ---------------------------
// 각 태스크는 무한 루프에서 LED 토글 및 딜레이 수행
// (딜레이는 busy-wait로 구현, 실제론 SVC로 sleep_ms() 호출하는 게 바람직)
// (여기선 단순화를 위해 busy-wait 사용)
// (task2는 1초마다 RTT 로그 출력, SVC 통계 확인용
pub extern "C" fn task0_entry() -> ! {
    loop {
        syscalls().gpio_toggle(os::GpioPin::Led1);
        for _ in 0..300 { cortex_m::asm::nop(); }
    }
}

pub extern "C" fn task1_entry() -> ! {
    loop {
        syscalls().gpio_toggle(os::GpioPin::Led2);
        for _ in 0..800 { cortex_m::asm::nop(); }
    }
}

pub extern "C" fn task2_entry() -> ! {
    // 1초마다 rtt 로그
    static mut LAST: u64 = 0;
    loop {
        let now = syscalls().now_ms();
        unsafe {
            if now.wrapping_sub(LAST) >= 1_000 {
                rtt_target::rprintln!("[task2] now={} ms", now);
                LAST = now;
            }
        }
        for _ in 0..1200 { cortex_m::asm::nop(); }
    }
}
// --------------------------- end Tasks ---------------------------


// --------------------------- main ---------------------------
const CYCLES_PER_MS_ESTIMATE: u32 = 16_000; // HSI 16 MHz (tune if needed)

// --- Unpriviledged Thread + PSP 전환용 유저 스택 (8바이트 정렬) ---
const STACK_BYTES: usize = 2048;
#[repr(align(8))]
struct UserStack([u8; 2048]);
static mut USER_STACK: UserStack = UserStack([0; STACK_BYTES]);  // size can be adjusted



#[inline(always)]
pub(crate) fn is_unpriv_thread() -> bool {
    let mut control: u32;
    unsafe {
        core::arch::asm!(
            "mrs {0}, CONTROL",
            out(reg) control,
        );
    }
    control & 1 != 0    // 1이면 Unpriviledged Thread 모드
}

// HardFault 핸들러: SP 전환 문제로 인한 오류 시 메시지 출력
use cortex_m_rt::exception;

#[exception]
unsafe fn HardFault(_ef: &cortex_m_rt::ExceptionFrame) -> ! {
    rtt_target::rprintln!("*** HardFault! (likely due to bad SP switch) ***");
    loop {}
}

#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("[mini-os] booting (button-controlled switching)");

    // Map both logical LEDs to PA5 for visibility; button is PC13.
    let mut board = board::BoardSyscalls::new(
        board::RawPin::new(board::GPIOA, 5),  // Led1 → PA5
        board::RawPin::new(board::GPIOA, 5),  // Led2 → PA5
        board::RawPin::new(board::GPIOC, 13), // Btn  → PC13
        CYCLES_PER_MS_ESTIMATE,
    );

    unsafe { board.init(); }
    rprintln!("GPIO ready: PA5 output, PC13 input-pullup");

    // SVC Handler에 커널 보드 포인터 등록
    unsafe { svc::register_kernel_board(&mut board as *mut _); }

    // Syscalls 클라이언트 생성
    let mut syscalls = unsafe { svc::Client::new(&mut board) };
    unsafe { SYSCALLS_PTR = &mut syscalls as *mut _; }
    
    // // Two apps: 0 = heartbeat, 1 = SOS
    // let mut app_beat = apps::HeartbeatApp::new(3);
    // let mut app_sos  = apps::LedSosApp::new();
    // let mut app_list: [&mut dyn os::App; 2] = [ &mut app_beat, &mut app_sos ];

    // let mut kernel = os::Os::new(&mut app_list, &mut syscalls);
    // // Button not pressed => heartbeat; pressed => SOS
    // kernel.run(os::AppCall::All);

    unsafe {
        sched::init_tasks();
        sched::init_systick_50us();
        rprintln!("Starting preemptive multitasking with {} tasks", sched::N_TASKS);
    }
    sched::start();
}