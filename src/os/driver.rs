use core::cell::RefCell;
use cortex_m::interrupt::{Mutex, CriticalSection};

struct GpioPin {
    state: bool,
}

impl GpioPin {
    const fn new() -> Self {
        Self { state: false }
    }

    fn on(&mut self) {
        self.state = true;
        // 실제 GPIO HIGH 처리 코드 (ex. GPIOx_ODR |= ...)
    }

    fn off(&mut self) {
        self.state = false;
        // 실제 GPIO LOW 처리
    }

    fn toggle(&mut self) {
        self.state = !self.state;
        // 실제 토글 처리
    }

    fn is_high(&self) -> bool {
        self.state
    }

    fn is_low(&self) -> bool {
        !self.state
    }
}

// ===============================
// === 글로벌 드라이버 객체들 ===
// ===============================

pub mod gpio {
    use super::*;
    
    // LED 드라이버 (singleton)
    pub static LED: Mutex<RefCell<GpioPin>> = Mutex::new(RefCell::new(GpioPin::new()));

    // 버튼 드라이버 (singleton)
    pub static BUTTON: Mutex<RefCell<ButtonPin>> = Mutex::new(RefCell::new(ButtonPin::new()));

    // Wrapper 타입 반환 (lock 시)
    pub struct GpioGuard<'cs> {
        pin: &'cs RefCell<GpioPin>,
        cs: &'cs CriticalSection,
    }

    impl<'cs> GpioGuard<'cs> {
        pub fn on(&self) {
            self.pin.borrow_mut().on();
        }

        pub fn off(&self) {
            self.pin.borrow_mut().off();
        }

        pub fn toggle(&self) {
            self.pin.borrow_mut().toggle();
        }
    }

    // 버튼 추상화
    pub struct ButtonPin {
        pressed: bool,
    }

    impl ButtonPin {
        const fn new() -> Self {
            Self { pressed: false }
        }

        pub fn is_pressed(&self) -> bool {
            // 실제 구현에선 GPIO 입력 판독
            self.pressed
        }
    }
}
