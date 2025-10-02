//! STM32F446 FreeRTOS-Style Mini-OS (Minimal Working Port)
//! - Implements SVC start, PendSV context switch, SysTick preemption
//! - Based on FreeRTOS Cortex-M4F port patterns

#![no_std]
#![no_main]
#![allow(warnings)]

use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rtt_init_print, rprintln};
use core::ffi::c_void;
use core::ptr::{read_volatile, write_volatile};
use core::arch::global_asm;

// ==== Config ====
pub const configCPU_CLOCK_HZ: u32 = 16_000_000;
pub const configTICK_RATE_HZ: u32 = 1000;
pub const configMAX_PRIORITIES: usize = 3;
pub const configMINIMAL_STACK_SIZE: usize = 64;

pub type StackType_t = u32;
pub type TaskFunction_t = unsafe extern "C" fn(*mut c_void) -> !;

#[repr(C)]
pub struct TCB {
    pub top_of_stack: *mut StackType_t,
}
impl TCB { pub const fn new() -> Self { Self { top_of_stack: core::ptr::null_mut() } } }

// ==== Globals ====
#[unsafe(no_mangle)]
pub static mut pxCurrentTCB: *mut TCB = core::ptr::null_mut();

static mut TASK_TCBS: [TCB; 3] = [TCB::new(), TCB::new(), TCB::new()];
static mut TASK_STACKS: [[StackType_t; configMINIMAL_STACK_SIZE]; 3] =
    [[0; configMINIMAL_STACK_SIZE]; 3];

static mut TASK_COUNT: usize = 0;
static mut CURRENT_INDEX: usize = 0;

// ==== Helper constants ====
pub const PORT_INITIAL_XPSR: u32 = 0x01000000; // Thumb bit
pub const PORT_START_ADDRESS_MASK: u32 = 0xFFFFFFFE;
pub const EXC_RETURN_THREAD_PSP: u32 = 0xFFFFFFFD;

const SCB_SHPR3: *mut u32 = 0xE000ED20 as *mut u32;
const SYSTICK_CSR: *mut u32 = 0xE000E010 as *mut u32;
const SYSTICK_RVR: *mut u32 = 0xE000E014 as *mut u32;
const SYSTICK_CVR: *mut u32 = 0xE000E018 as *mut u32;

const CPACR: *mut u32 = 0xE000ED88 as *mut u32;
const FPCCR: *mut u32 = 0xE000EF34 as *mut u32;

// ==== ASM Handlers ====
global_asm!(
    r#"
    .syntax unified
    .thumb

    .global SVC_Handler
    .type   SVC_Handler,%function
    .thumb_func
SVC_Handler:
    ldr r0, =pxCurrentTCB
    ldr r0, [r0]
    ldr r0, [r0]
    msr psp, r0
    movs r0, #2
    msr CONTROL, r0
    isb
    pop {{r0-r5}}   // dummy clear
    mov lr, #0xFFFFFFFD
    bx lr

    .global PendSV_Handler
    .type PendSV_Handler,%function
    .thumb_func
PendSV_Handler:
    mrs r0, psp
    cbz r0, pend_first

    stmdb r0!, {{r4-r11}}
    ldr r1, =pxCurrentTCB
    ldr r2, [r1]
    str r0, [r2]

pend_switch:
    bl vTaskSwitchContext

    ldr r1, =pxCurrentTCB
    ldr r2, [r1]
    ldr r0, [r2]
    ldmia r0!, {{r4-r11}}
    msr psp, r0
    bx lr

pend_first:
    b pend_switch
"#
);

// ==== Port Functions ====
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vTaskSwitchContext() {
    let next = (CURRENT_INDEX + 1) % TASK_COUNT;
    CURRENT_INDEX = next;
    pxCurrentTCB = &mut TASK_TCBS[next];
}

unsafe fn px_port_initialise_stack(
    top: *mut StackType_t,
    entry: TaskFunction_t,
    param: *mut c_void,
) -> *mut StackType_t {
    let mut sp = top;
    // Hardware frame
    sp = sp.offset(-1); *sp = PORT_INITIAL_XPSR;
    sp = sp.offset(-1); *sp = (entry as u32 & PORT_START_ADDRESS_MASK);
    sp = sp.offset(-1); *sp = task_exit_error as u32;
    sp = sp.offset(-1); *sp = 0; // R12
    sp = sp.offset(-1); *sp = 0; // R3
    sp = sp.offset(-1); *sp = 0; // R2
    sp = sp.offset(-1); *sp = 0; // R1
    sp = sp.offset(-1); *sp = param as u32; // R0
    // Software frame
    for _ in 0..8 {
        sp = sp.offset(-1); *sp = 0;
    }
    sp
}

#[unsafe(no_mangle)]
pub extern "C" fn task_exit_error() -> ! {
    rprintln!("[ERROR] Task returned!");
    loop { cortex_m::asm::bkpt(); }
}

unsafe fn prv_setup_timer_interrupt() {
    let reload = configCPU_CLOCK_HZ / configTICK_RATE_HZ;
    write_volatile(SYSTICK_RVR, reload - 1);
    write_volatile(SYSTICK_CVR, 0);
    write_volatile(SYSTICK_CSR, 0x07);
}

// ==== API ====
unsafe fn xTaskCreate(
    entry: TaskFunction_t,
    _name: &str,
    handle: &mut *mut c_void,
) {
    let idx = TASK_COUNT;
    let stack_end = TASK_STACKS[idx].as_mut_ptr().add(configMINIMAL_STACK_SIZE - 1);
    TASK_TCBS[idx].top_of_stack =
        px_port_initialise_stack(stack_end, entry, core::ptr::null_mut());
    *handle = &mut TASK_TCBS[idx] as *mut _ as *mut c_void;
    TASK_COUNT += 1;
}

unsafe fn vTaskStartScheduler() -> ! {
    // Enable FPU + lazy stacking
    let mut cpacr = read_volatile(CPACR);
    cpacr |= 0xF << 20;
    write_volatile(CPACR, cpacr);
    write_volatile(FPCCR, (1<<31) | (1<<30)); // ASPEN+LSPEN

    // Priorities: SysTick[31:24], PendSV[23:16]
    let mut shpr3 = read_volatile(SCB_SHPR3);
    shpr3 &= !0xFF00_0000;
    shpr3 &= !0x00FF_0000;
    shpr3 |= (0xFF << 24) | (0xFF << 16);
    write_volatile(SCB_SHPR3, shpr3);

    prv_setup_timer_interrupt();

    pxCurrentTCB = &mut TASK_TCBS[0];
    CURRENT_INDEX = 0;

    // Trigger SVC to start first task
    core::arch::asm!("svc 0");
    loop {}
}

// ==== Tasks ====
#[unsafe(no_mangle)]
pub extern "C" fn task0(_: *mut c_void) -> ! {
    loop {
        rprintln!("[TASK0] running");
        for _ in 0..1_000 { cortex_m::asm::nop(); }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn task1(_: *mut c_void) -> ! {
    loop {
        rprintln!("[TASK1] running");
        for _ in 0..1_000 { cortex_m::asm::nop(); }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn task2(_: *mut c_void) -> ! {
    loop {
        rprintln!("[TASK2] running");
        for _ in 0..1_000 { cortex_m::asm::nop(); }
    }
}

// ==== SysTick ====
#[unsafe(no_mangle)]
pub extern "C" fn SysTick() {
    cortex_m::peripheral::SCB::set_pendsv();
}

// ==== Entry ====
#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("[BOOT] Starting mini FreeRTOS-style OS");

    unsafe {
        let mut h0: *mut c_void = core::ptr::null_mut();
        let mut h1: *mut c_void = core::ptr::null_mut();
        let mut h2: *mut c_void = core::ptr::null_mut();
        xTaskCreate(task0, "T0", &mut h0);
        xTaskCreate(task1, "T1", &mut h1);
        xTaskCreate(task2, "T2", &mut h2);

        vTaskStartScheduler();
    }
}