// mini_os_app_framework.rs (Button-controlled app switching, STM32F446)
// - Board: STM32F446 (e.g., Nucleo-F446RE)
#![no_std]
#![no_main]
#![allow(dead_code)]


use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};

// ------------------------- SVC layer ------------------------
mod svc {
    // --------------------- for rtt debug ----------------------
    use core::sync::atomic::{ AtomicU32, Ordering };

    static SVC_COUNTER: AtomicU32 = AtomicU32::new(0);
    static NOW_COUNT: AtomicU32 = AtomicU32::new(0);
    static BTN_COUNT: AtomicU32 = AtomicU32::new(0);

    pub fn svc_stats() -> (u32, u32, u32) {
        (
            SVC_COUNTER.load(Ordering::Relaxed),
            NOW_COUNT.load(Ordering::Relaxed),
            BTN_COUNT.load(Ordering::Relaxed),
        )
    }
    // --------------------- end rtt debug ----------------------

    use core::arch::{ asm, global_asm };
    use crate::os::Syscalls;

    // --------- ABI : call_id definitions ----------
    pub mod abi {
        pub const NOW_MS: u8 = 1;
        pub const BTN_PRESSED: u8 = 2;
        pub const GPIO_WRITE: u8 = 3;
        pub const GPIO_TOGGLE: u8 = 4;
        pub const SLEEP_MS: u8 = 5;
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
            abi::BTN_PRESSED => { BTN_COUNT.fetch_add(1, Ordering::Relaxed); }
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

    // --------- (5) 실제 디스패처: 지금은 NOW_MS만 처리 ----------
    unsafe fn kernel_dispatch(call_id: u8, a0: u32, a1: u32, _a2: u32, _a3: u32) -> u32 {
        let board = unsafe { &mut *BOARD_PTR };
        match call_id {
            abi::NOW_MS => board.now_ms() as u32,

            abi::BTN_PRESSED => {
                if board.user_button_pressed() { 1 } else { 0 }
            },

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
        fn user_button_pressed(&self) -> bool {
            svc_call(abi::BTN_PRESSED, 0, 0, 0, 0) != 0
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
        fn user_button_pressed(&self) -> bool; // ← Board input exposed as a syscall
    }

    pub trait App {
        fn name(&self) -> &'static str;
        fn init(&mut self, _sys: &mut dyn Syscalls) {}
        fn tick(&mut self, sys: &mut dyn Syscalls);
    }

    pub enum AppCall<'a> {
        ByName(&'a str),
        ByIndex(usize),
        All,
        /// Run `primary` app while button is released; run `secondary` while pressed.
        SwitchOnButton { primary: usize, secondary: usize },
    }

    pub struct Os<'a> {
        apps: &'a mut [&'a mut dyn App],
        sys: &'a mut dyn Syscalls,
        started: bool,
    }

    impl<'a> Os<'a> {
        pub fn new(apps: &'a mut [&'a mut dyn App], sys: &'a mut dyn Syscalls) -> Self {
            Self { apps, sys, started: false }
        }
        pub fn run(&'a mut self, call: AppCall<'a>) -> ! {
            // One-time init for all apps
            if !self.started { for a in self.apps.iter_mut() { a.init(self.sys); } self.started = true; }

            assert!(!self.apps.is_empty(), "no apps to run");

            match call {
                AppCall::ByIndex(mut i) => {
                    i %= self.apps.len();
                    loop { self.apps[i].tick(self.sys); }
                }
                AppCall::ByName(name) => {
                    let mut idx = 0usize;
                    for (i, a) in self.apps.iter().enumerate() { if a.name() == name { idx = i; break; } }
                    loop { self.apps[idx].tick(self.sys); }
                }
                AppCall::All => {
                    loop { for a in self.apps.iter_mut() { a.tick(self.sys); } }
                }
                AppCall::SwitchOnButton { mut primary, mut secondary } => {
                    let len = self.apps.len();
                    primary %= len; secondary %= len;
                    loop {
                        if self.sys.user_button_pressed() {
                            self.apps[secondary].tick(self.sys);
                        } else {
                            self.apps[primary].tick(self.sys);
                        }
                        self.sys.sleep_ms(1); // debounce / cooperative yield
                    }
                }
            }
        }
    }
}

// ------------------------- Apps ----------------------------
mod apps {
    use super::os::{App, GpioPin, Syscalls};
    use rtt_target::rprintln;

    /// Heartbeat: steady blink on PA5 (both Led1/Led2 mapped) to show liveness.
    pub struct HeartbeatApp { last: u64, on: bool, period_ms: u32, dbg_last_log: u64 }
    impl HeartbeatApp {
        pub const fn new(period_ms: u32) -> Self {
             Self { last: 0, on: false, period_ms, dbg_last_log: 0 } 
            } 
    }
    impl App for HeartbeatApp {
        fn name(&self) -> &'static str { "heartbeat" }
        fn tick(&mut self, sys: &mut dyn Syscalls) {
            let now = sys.now_ms();
            if now.wrapping_sub(self.last) >= self.period_ms as u64 {
                self.on = !self.on;
                sys.gpio_write(GpioPin::Led1, self.on);
                sys.gpio_write(GpioPin::Led2, self.on);
                self.last = now;
            }

            // ---- for rtt debug: 1초마다 SVC 통계 출력 ---
            if now.wrapping_sub(self.dbg_last_log) >= 1000 {
                let (svc, nowc, btnc) = crate::svc::svc_stats();
                rprintln!("SVC hits: total={}, now_ms={}, btn={}", svc, nowc, btnc);
                self.dbg_last_log = now;
            }
            // ------------------------------------------

            sys.sleep_ms(1);
        }
    }

    /// SOS pattern on PA5: ··· ––– ···, repeats
    pub struct LedSosApp;
    impl LedSosApp { pub const fn new() -> Self { Self } }
    impl App for LedSosApp {
        fn name(&self) -> &'static str { "led_sos" }
        fn tick(&mut self, sys: &mut dyn Syscalls) {
            const DOT: u32 = 2; const DASH: u32 = 6; const GAP: u32 = 2; const WORD: u32 = 7;
            let mut pulse = |dur: u32| {
                sys.gpio_write(GpioPin::Led2, true);  sys.sleep_ms(dur);
                sys.gpio_write(GpioPin::Led2, false); sys.sleep_ms(GAP);
            };
            for _ in 0..2 { pulse(DOT); }
            for _ in 0..2 { pulse(DASH); }
            for _ in 0..2 { pulse(DOT); }
            sys.sleep_ms(WORD);
        }
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
        fn user_button_pressed(&self) -> bool {
            unsafe {
                // B1 on Nucleo-F446RE (PC13): pull-up. Pressed => level LOW.
                let high = gpio_read_input(self.btn.port_base, self.btn.pin);
                !high
            }
        }
    }

    pub const GPIOA: u32 = GPIOA_BASE;
    pub const GPIOC: u32 = GPIOC_BASE;
}

// --------------------------- main ---------------------------
const CYCLES_PER_MS_ESTIMATE: u32 = 16_000; // HSI 16 MHz (tune if needed)

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
    
    // Two apps: 0 = heartbeat, 1 = SOS
    let mut app_beat = apps::HeartbeatApp::new(3);
    let mut app_sos  = apps::LedSosApp::new();
    let mut app_list: [&mut dyn os::App; 2] = [ &mut app_beat, &mut app_sos ];

    let mut kernel = os::Os::new(&mut app_list, &mut syscalls);
    // Button not pressed => heartbeat; pressed => SOS
    kernel.run(os::AppCall::SwitchOnButton { primary: 0, secondary: 1 })
}