#![no_std]
#![no_main]

use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};
use stm32f4xx_hal as hal;

use hal::{
    gpio::{gpioa::PA5, Output, PushPull},
    prelude::*,
};

use rtic::app;

#[app(device = stm32f4xx_hal::pac, peripherals = true)]
mod app {
    use super::*;

    #[shared]
    struct Shared {
        led: PA5<Output<PushPull>>,
    }

    #[local]
    struct Local {
        counter0: u32,
        counter1: u32,
        counter2: u32,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local) {
        rtt_init_print!();

        // RTT delay workaround
        for _ in 0..100_000 {
            cortex_m::asm::nop();
        }

        rprintln!("[RTIC] Booting on STM32F446...");

        let rcc = ctx.device.RCC.constrain();
        let clocks = rcc.cfgr.sysclk(16.MHz()).freeze();

        let gpioa = ctx.device.GPIOA.split();
        let led = gpioa.pa5.into_push_pull_output();

        // SysTick every 1s
        let mut syst = ctx.core.SYST;
        syst.set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
        syst.set_reload(clocks.sysclk().raw()); // 16 MHz → 1s
        syst.clear_current();
        syst.enable_interrupt();
        syst.enable_counter();

        rprintln!("[RTIC] Init done.");

        (
            Shared { led },
            Local {
                counter0: 0,
                counter1: 0,
                counter2: 0,
            },
        )
    }

    // ───── TASK 0 ─────
    #[task(shared = [led], local = [counter0])]
    async fn task0(mut ctx: task0::Context) {
        *ctx.local.counter0 += 1;
        ctx.shared.led.lock(|led| {
            led.set_high();
        });

        rprintln!("[TASK0] LED ON (cnt: {})", ctx.local.counter0);
    }

    // ───── TASK 1 ─────
    #[task(local = [counter1])]
    async fn task1(ctx: task1::Context) {
        *ctx.local.counter1 += 1;

        if *ctx.local.counter1 % 50 == 0 {
            rprintln!("[TASK1] Running... (cnt: {})", ctx.local.counter1);
        }
    }

    // ───── TASK 2 ─────
    #[task(local = [counter2])]
    async fn task2(ctx: task2::Context) {
        *ctx.local.counter2 += 1;

        if *ctx.local.counter2 % 30 == 0 {
            rprintln!("[TASK2] Running... (cnt: {})", ctx.local.counter2);
        }
    }

    // ───── SCHEDULER VIA SYSTICK ─────
    #[task(binds = SysTick, priority = 1)]
    fn systick(_ctx: systick::Context) {
        static mut ROTATE: u8 = 0;
        static mut PREV_TASK: &'static str = "IDLE";

        let rotate = unsafe { &mut ROTATE };
        let prev_task = unsafe { &mut PREV_TASK };

        let next_task = match *rotate {
            0 => {
                rprintln!("[SCHEDULER] Context Switch: {} -> TASK0", *prev_task);
                task0::spawn().ok();
                "TASK0"
            }
            1 => {
                rprintln!("[SCHEDULER] Context Switch: {} -> TASK1", *prev_task);
                task1::spawn().ok();
                "TASK1"
            }
            2 => {
                rprintln!("[SCHEDULER] Context Switch: {} -> TASK2", *prev_task);
                task2::spawn().ok();
                "TASK2"
            }
            _ => *prev_task,
        };

        *prev_task = next_task;
        *rotate = (*rotate + 1) % 3;
    }
}
