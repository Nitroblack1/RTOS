pub mod kernel;
pub mod task;
pub mod driver;

use kernel::Kernel;
use task::{Task, TaskId};

pub fn os_main() -> ! {
    let mut kernel = Kernel::new();

    // 태스크 등록
    // 나중에 optimize해줘야 함. 확장성 확보
    kernel.add_task(Task::new(TaskId(1), task1_entry));
    kernel.add_task(Task::new(TaskId(2), task2_entry));
    kernel.add_task(Task::new(TaskId(3), task3_entry));

    // 커널 실행
    kernel.run();
}

// ex. HeartBeat
fn task1_entry() {
    // use driver::gpio::LED;
    // use cortex_m::asm::delay;
}

// ex. SOS
fn task2_entry() {
    // use driver::gpio::LED;
    // use cortex_m::asm::delay;
}


// ex. BTN pressed LED
fn task3_entry() {
    // use driver::gpio::{LED, BUTTON};
    // use cortex_m::asm::delay;
}
