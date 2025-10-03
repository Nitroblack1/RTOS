//! STM32F446 FreeRTOS-Style Mini-OS (Minimal Working Port)
//! - SVC: first task start (restore r4–r11, r14; set PSP, EXC_RETURN)
//! - PendSV: FreeRTOS-style context switch incl. optional FPU lazy stacking
//! - SysTick: preemption by pendsv set
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

// 태스크 스택: 256워드 = 1024바이트 (필요시 512로 확대 가능)
pub const configMINIMAL_STACK_SIZE: usize = 256;

pub type StackType_t = u32;
pub type TaskFunction_t = unsafe extern "C" fn(*mut c_void) -> !;

// FreeRTOS-style: BASEPRI로 마스킹할 최대 시스템콜 인터럽트 우선도
pub const configMAX_SYSCALL_INTERRUPT_PRIORITY: u32 = 0x20;

#[repr(C)]
pub struct TCB {
    pub top_of_stack: *mut StackType_t,
}
impl TCB { pub const fn new() -> Self { Self { top_of_stack: core::ptr::null_mut() } } }

// ==== Globals ====
#[unsafe(no_mangle)]
pub static mut pxCurrentTCB: *mut TCB = core::ptr::null_mut();

#[repr(align(8))]
#[derive(Copy, Clone)]
struct AlignedStack([StackType_t; configMINIMAL_STACK_SIZE]);

static mut TASK_TCBS: [TCB; 3] = [TCB::new(), TCB::new(), TCB::new()];
static mut TASK_STACKS: [AlignedStack; 3] =
    [AlignedStack([0; configMINIMAL_STACK_SIZE]); 3];

static mut TASK_COUNT: usize = 0;
static mut CURRENT_INDEX: usize = 0;

// ==== Helper constants ====
pub const PORT_INITIAL_XPSR: u32 = 0x0100_0000; // Thumb bit
pub const PORT_START_ADDRESS_MASK: u32 = 0xFFFF_FFFE;

const SCB_SHPR3: *mut u32 = 0xE000_ED20 as *mut u32; // [31:24] SysTick, [23:16] PendSV
const SYSTICK_CSR: *mut u32 = 0xE000_E010 as *mut u32;
const SYSTICK_RVR: *mut u32 = 0xE000_E014 as *mut u32;
const SYSTICK_CVR: *mut u32 = 0xE000_E018 as *mut u32;

const CPACR: *mut u32 = 0xE000_ED88 as *mut u32;
const FPCCR: *mut u32 = 0xE000_EF34 as *mut u32;

// ==== ASM Handlers ====
// SVC: 첫 태스크 기동 (SW 프레임 r4-r11, r14 복구 → PSP 세팅 → EXC_RETURN)
global_asm!(
    r#"
    .syntax unified
    .thumb

    .global SVCall
    .type   SVCall,%function
    .thumb_func
SVCall:
    /* r1 = &pxCurrentTCB; r1 = pxCurrentTCB; r0 = pxCurrentTCB->top_of_stack */
    ldr     r1, =pxCurrentTCB
    ldr     r1, [r1]
    ldr     r0, [r1]

    /* 소프트웨어 컨텍스트 복구 (r4-r11 + r14) */
    ldmia   r0!, {{r4-r11, r14}}

    /* PSP = 하드웨어 프레임 시작 주소 */
    msr     psp, r0

    /* Thread mode를 PSP로 (필요시 nPRIV=1 사용: #3) */
    movs    r0, #2         /* CONTROL.SPSEL=1 (PSP), nPRIV=0 */
    msr     CONTROL, r0
    isb

    /* EXC_RETURN(Thread/PSP) */
    ldr     r0, =0xFFFFFFFD
    bx      r0
"#
);

// PendSV: FreeRTOS Cortex-M4F 포트와 동일한 컨텍스트 스위치
global_asm!(
    r#"
    .syntax unified
    .thumb

    .global PendSV
    .type   PendSV,%function
    .thumb_func
PendSV:
    mrs r0, psp
    isb

    /* r3 = &pxCurrentTCB; r2 = pxCurrentTCB */
    ldr r3, =pxCurrentTCB
    ldr r2, [r3]

    /* FPU 상위 레지스터 사용 여부 검사 (EXC_RETURN bit[4]) */
    tst r14, #0x10
    it eq
    vstmdbeq r0!, {{s16-s31}}

    /* 코어 레지스터 + LR 저장 */
    stmdb r0!, {{r4-r11, r14}}
    /* TCB->top_of_stack = r0 */
    str r0, [r2]

    /* 임계구역: BASEPRI 마스크 올리기 */
    stmdb sp!, {{r0, r3}}
    mov r0, {prio}
    msr basepri, r0
    dsb
    isb
    bl vTaskSwitchContext
    mov r0, #0
    msr basepri, r0
    ldmia sp!, {{r0, r3}}

    /* 새 TCB */
    ldr r1, [r3]
    ldr r0, [r1]

    /* 코어 레지스터 + LR 복구 */
    ldmia r0!, {{r4-r11, r14}}

    /* FPU 상위 레지스터 복구 필요? */
    tst r14, #0x10
    it eq
    vldmiaeq r0!, {{s16-s31}}

    /* PSP 업데이트 후 복귀 */
    msr psp, r0
    isb
    bx r14
"#
, prio = const configMAX_SYSCALL_INTERRUPT_PRIORITY);

// ==== Fault Handlers (디버깅용) ====
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HardFault() -> ! {
    rprintln!("[Hard Fault]");
    loop {}
}

// ==== Port Functions ====
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vTaskSwitchContext() {
    let next = (CURRENT_INDEX + 1) % TASK_COUNT;
    CURRENT_INDEX = next;
    pxCurrentTCB = &mut TASK_TCBS[next];
}

const EXC_RETURN_THREAD_PSP: u32 = 0xFFFF_FFFD;

unsafe fn px_port_initialise_stack(
    top: *mut StackType_t,
    entry: TaskFunction_t,
    param: *mut c_void,
) -> *mut StackType_t {
    let mut sp = top;

    // ----- 하드웨어 프레임 -----
    sp = sp.offset(-1); *sp = PORT_INITIAL_XPSR;                        // xPSR
    sp = sp.offset(-1); *sp = (entry as u32 & PORT_START_ADDRESS_MASK); // PC
    sp = sp.offset(-1); *sp = task_exit_error as u32;                   // LR (함수 리턴 방지)
    sp = sp.offset(-1); *sp = 0;                                        // R12
    sp = sp.offset(-1); *sp = 0;                                        // R3
    sp = sp.offset(-1); *sp = 0;                                        // R2
    sp = sp.offset(-1); *sp = 0;                                        // R1
    sp = sp.offset(-1); *sp = param as u32;                             // R0

    // ----- 소프트웨어 프레임 (r4-r11 + LR) -----
    // r4~r11 = 0
    for _ in 0..8 {
        sp = sp.offset(-1);
        *sp = 0;
    }
    // r14(LR) = EXC_RETURN(Thread/PSP)
    sp = sp.offset(-1);
    *sp = EXC_RETURN_THREAD_PSP;

    sp
}


#[unsafe(no_mangle)]
pub unsafe extern "C" fn task_exit_error() -> ! {
    rprintln!("[ERROR] Task returned!]");
    loop { cortex_m::asm::bkpt(); }
}

unsafe fn prv_setup_timer_interrupt() {
    rprintln!("timer started");
    let reload = configCPU_CLOCK_HZ / configTICK_RATE_HZ;
    write_volatile(SYSTICK_RVR, reload - 1);
    write_volatile(SYSTICK_CVR, 0);
    write_volatile(SYSTICK_CSR, 0x07); // enable | tickint | processor clock
}

// ==== API ====
unsafe fn xTaskCreate(
    entry: TaskFunction_t,
    _name: &str,
    handle: &mut *mut c_void,
) {
    let idx = TASK_COUNT;
    let stack_end = (*(&mut TASK_STACKS[idx].0)).as_mut_ptr().add(configMINIMAL_STACK_SIZE - 1);
    TASK_TCBS[idx].top_of_stack =
        px_port_initialise_stack(stack_end, entry, core::ptr::null_mut());
    *handle = &mut TASK_TCBS[idx] as *mut _ as *mut c_void;
    TASK_COUNT += 1;
}

unsafe fn vTaskStartScheduler() -> ! {
    rprintln!("scheduler entered");

    // === FPU Enable (vPortEnableVFP 패턴) ===
    let mut cpacr = read_volatile(CPACR);
    cpacr |= 0xF << 20;  // CP10, CP11 enable
    write_volatile(CPACR, cpacr);

    // Lazy stacking: ASPEN | LSPEN
    write_volatile(FPCCR, (1 << 31) | (1 << 30));

    // PendSV/SysTick 우선도 최하 (필요시 미세조정 OK)
    let mut shpr3 = read_volatile(SCB_SHPR3);
    shpr3 &= !0xFF00_0000;
    shpr3 &= !0x00FF_0000;
    shpr3 |= (0xFF << 24) | (0xFF << 16); // SysTick=0xFF, PendSV=0xFF
    write_volatile(SCB_SHPR3, shpr3);

    prv_setup_timer_interrupt();

    // 첫 태스크 선택
    pxCurrentTCB = &mut TASK_TCBS[0];
    CURRENT_INDEX = 0;

    rprintln!("first task about to start");
    core::arch::asm!("svc 0");
    loop {}
}

// ==== Tasks ====
#[unsafe(no_mangle)]
pub unsafe extern "C" fn task0(_: *mut c_void) -> ! {
    loop {
        rprintln!("[TASK0] running");
        for _ in 0..1_000 { cortex_m::asm::nop(); }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn task1(_: *mut c_void) -> ! {
    loop {
        rprintln!("[TASK1] running");
        for _ in 0..1_000 { cortex_m::asm::nop(); }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn task2(_: *mut c_void) -> ! {
    loop {
        rprintln!("[TASK2] running");
        for _ in 0..1_000 { cortex_m::asm::nop(); }
    }
}

// ==== SysTick ====
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SysTick() {
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
