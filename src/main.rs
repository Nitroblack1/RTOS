#![no_std]
#![no_main]

use cortex_m_rt::entry;
use panic_halt as _;
use cortex_m::peripheral::{Peripherals, SYST};
use stm32f4::stm32f446;

// RTT 로깅 지원
use rtt_target::{rtt_init_print, rprintln};

// ------------- Traits -------------
trait Driver {
    fn command(&self);
}

trait Platform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn Driver>) -> R;
}

trait Chip {
    fn mpu(&self);
    fn systick(&mut self, syst: &mut SYST, sysclk_hz: u32);
    fn userspace_kernel_boundary(&self);
}

// ------------ Components ------------

struct MockDriver;
impl Driver for MockDriver {
    fn command(&self) {
        rprintln!("Driver command executed");
    }
}

struct MyPlatform;
impl Platform for MyPlatform {
    fn with_driver<F, R>(&self, driver_num: usize, f: F) -> R
    where
        F: FnOnce(Option<&dyn Driver>) -> R,
    {
        if driver_num == 1 {
            let driver = MockDriver;
            f(Some(&driver))
        } else {
            f(None)
        }
    }
}

struct MyChip;
impl Chip for MyChip {
    fn mpu(&self) {
        rprintln!("[CHIP] MPU 초기화 (mock)");
    }

    fn systick(&mut self, syst: &mut SYST, sysclk_hz: u32) {
        syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
        syst.set_reload(sysclk_hz); // 1초
        syst.clear_current();
        syst.enable_counter();
        rprintln!("[CHIP] SysTick 설정 완료");
    }

    fn userspace_kernel_boundary(&self) {
        rprintln!("[CHIP] 유저스페이스 경계 설정 (mock)");
    }
}

// ------------ Process ------------
struct Process {
    id: usize,
    name: &'static str,
}

impl Process {
    fn new(id: usize, name: &'static str) -> Self {
        Process { id, name }
    }

    fn run(&self) {
        rprintln!("[PROCESS] {} 실행 중...", self.name);
    }
}

// ------------ Kernel ------------
struct Kernel {
    processes: [Option<Process>; 4],
}

impl Kernel {
    fn new() -> Self {
        Kernel {
            processes: [None, None, None, None],
        }
    }

    fn load_processes(&mut self) {
        self.processes[0] = Some(Process::new(0, "console"));
        self.processes[1] = Some(Process::new(1, "gpio"));
        rprintln!("[KERNEL] 프로세스 로딩 완료");
    }

    fn main_loop(&self, gpio: &mut GpioPin) -> ! {
        rprintln!("[KERNEL] 메인 루프 진입");

        loop {
            for process in self.processes.iter().flatten() {
                process.run();
                gpio.toggle();
                delay(8_000_000); // 약 1초 (8 MHz 기준)
            }
        }
    }
}

// ------------ GPIO 제어 (PAC 기반) ------------
struct GpioPin {
    port: &'static stm32f446::GPIOC,
    pin: u8,
}

impl GpioPin {
    fn new(dp: &stm32f446::Peripherals) -> Self {
        // RCC enable
        dp.RCC.ahb1enr.modify(|_, w| w.gpiocen().enabled());

        // GPIOC13: Push-pull output
        let gpio = &dp.GPIOC;
        unsafe {
            gpio.moder.modify(|r, w| {
                let mut bits = r.bits();
                bits &= !(0b11 << (13 * 2)); // clear bits
                bits |= 0b01 << (13 * 2);    // set output
                w.bits(bits)
            });

            gpio.otyper.modify(|r, w| {
                let mut bits = r.bits();
                bits &= !(1 << 13); // push-pull
                w.bits(bits)
            });
        }

        GpioPin {
            port: gpio,
            pin: 13,
        }
    }

    fn toggle(&self) {
        if (self.port.odr.read().bits() & (1 << self.pin)) != 0 {
            self.port.bsrr.write(|w| unsafe { w.bits(1 << (self.pin + 16)) }); // reset
        } else {
            self.port.bsrr.write(|w| unsafe { w.bits(1 << self.pin) }); // set
        }
    }
}

// ------------ crude delay ------------
fn delay(cycles: u32) {
    for _ in 0..cycles {
        cortex_m::asm::nop();
    }
}

// ------------ Entry ------------
#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("[BOOT] 시스템 부팅 시작");

    let cp = Peripherals::take().unwrap();
    let dp = stm32f446::Peripherals::take().unwrap();

    let sysclk_hz = 8_000_000; // 외부 크리스탈 없이 기본값 가정

    // 초기화
    let mut chip = MyChip;
    chip.mpu();
    chip.systick(&mut cp.SYST, sysclk_hz);
    chip.userspace_kernel_boundary();

    let mut gpio = GpioPin::new(&dp);

    let mut kernel = Kernel::new();
    kernel.load_processes();

    kernel.main_loop(&mut gpio)
}
