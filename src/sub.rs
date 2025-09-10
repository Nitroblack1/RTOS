// mini_os_app_framework.rs (Button-controlled app switching, STM32F446)
// - Board: STM32F446 (e.g., Nucleo-F446RE)
// - LEDs: Map BOTH logical LEDs to PA5 (LD2) so everything is visible.
// - Button: Use user button B1 on PC13 with pull-up; pressed = LOW.
// - Clock: assume HSI 16 MHz; adjust CYCLES_PER_MS_ESTIMATE as needed.
#![no_std]
#![no_main]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(non_snake_case)]


use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};

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

    /// Heartbeat: steady blink on PA5 (both Led1/Led2 mapped) to show liveness.
    pub struct HeartbeatApp { last: u64, on: bool, period_ms: u32 }
    impl HeartbeatApp { pub const fn new(period_ms: u32) -> Self { Self { last: 0, on: false, period_ms } } }
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
    let mut syscalls = board::BoardSyscalls::new(
        board::RawPin::new(board::GPIOA, 5),  // Led1 → PA5
        board::RawPin::new(board::GPIOA, 5),  // Led2 → PA5
        board::RawPin::new(board::GPIOC, 13), // Btn  → PC13
        CYCLES_PER_MS_ESTIMATE,
    );

    unsafe { syscalls.init(); }
    rprintln!("GPIO ready: PA5 output, PC13 input-pullup");

    // Two apps: 0 = heartbeat, 1 = SOS
    let mut app_beat = apps::HeartbeatApp::new(3);
    let mut app_sos  = apps::LedSosApp::new();
    let mut app_list: [&mut dyn os::App; 2] = [ &mut app_beat, &mut app_sos ];

    let mut kernel = os::Os::new(&mut app_list, &mut syscalls);
    // Button not pressed => heartbeat; pressed => SOS
    kernel.run(os::AppCall::SwitchOnButton { primary: 0, secondary: 1 })
}