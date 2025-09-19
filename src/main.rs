//! STM32F446 Mini-OS - Monolithic Implementation
//!
//! A complete mini operating system implementation in a single file
//! with multitasking, context switching, and hardware abstraction.

#![no_std]
#![no_main]
#![allow(dead_code)]

use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};
use core::arch::global_asm;
use core::ptr::{read_volatile, write_volatile};

// ===== Configuration =====

/// System clock frequency (16 MHz HSI)
pub const SYSTEM_CLOCK_HZ: u32 = 16_000_000;

/// Maximum number of tasks
pub const MAX_TASKS: usize = 8;

/// Task stack size in words (1KB per task)
pub const TASK_STACK_SIZE_WORDS: usize = 256;

/// Kernel stack size in words (1KB)
pub const KERNEL_STACK_SIZE_WORDS: usize = 256;

/// Software context size (r4-r11)
pub const SW_CONTEXT_SIZE: usize = 8;

/// Hardware context size (r0-r3, r12, lr, pc, xpsr)
pub const HW_CONTEXT_SIZE: usize = 8;

/// Stack guard size
pub const STACK_GUARD_SIZE: usize = 4;

/// CPU cycles per millisecond
pub const CYCLES_PER_MS: u32 = SYSTEM_CLOCK_HZ / 1000;

/// Memory layout
pub const SRAM_START: u32 = 0x2000_0000;
pub const SRAM_SIZE: u32 = 128 * 1024; // 128KB
pub const SRAM_END: u32 = SRAM_START + SRAM_SIZE;

/// Interrupt priorities
pub const PENDSV_PRIORITY: u8 = 0xFF; // Lowest priority
pub const SYSTICK_PRIORITY: u8 = 0xFE;

/// SysTick reload value for 10ms period
pub const SYSTICK_RELOAD_10MS: u32 = (SYSTEM_CLOCK_HZ / 100) - 1;

/// Time slice per task in milliseconds
pub const TIME_SLICE_MS: u32 = 10;

// ===== Register Addresses =====

/// NVIC ICSR register
pub const NVIC_ICSR: u32 = 0xE000_ED04;

/// SCB SHPR3 register
pub const SCB_SHPR3: u32 = 0xE000_ED20;

/// SysTick registers
pub const SYSTICK_CSR: u32 = 0xE000_E010;
pub const SYSTICK_RVR: u32 = 0xE000_E014;
pub const SYSTICK_CVR: u32 = 0xE000_E018;

/// RCC registers
pub const RCC_AHB1ENR: u32 = 0x4002_3830;

/// GPIO registers
pub const GPIOA_BASE: u32 = 0x4002_0000;
pub const GPIOC_BASE: u32 = 0x4002_0800;

pub const GPIO_MODER_OFFSET: u32 = 0x00;
pub const GPIO_OTYPER_OFFSET: u32 = 0x04;
pub const GPIO_OSPEEDR_OFFSET: u32 = 0x08;
pub const GPIO_PUPDR_OFFSET: u32 = 0x0C;
pub const GPIO_IDR_OFFSET: u32 = 0x10;
pub const GPIO_ODR_OFFSET: u32 = 0x14;
pub const GPIO_BSRR_OFFSET: u32 = 0x18;

/// Pin definitions
pub const LED_PIN: u8 = 5; // PA5
pub const BUTTON_PIN: u8 = 13; // PC13

// ===== Process Management =====

/// Process Control Block (PCB)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ProcessControlBlock {
    /// Process stack pointer
    pub stack_pointer: u32,
    /// Process ID
    pub process_id: usize,
    /// Process state
    pub state: ProcessState,
}

/// Process states
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

/// Stack structure with alignment
#[repr(align(8))]
#[derive(Copy, Clone)]
pub struct ProcessStack([u32; TASK_STACK_SIZE_WORDS]);

/// Kernel stack structure
#[repr(align(8))]
#[derive(Copy, Clone)]
pub struct KernelStack([u32; KERNEL_STACK_SIZE_WORDS]);

/// Process manager
pub struct ProcessManager {
    processes: [ProcessControlBlock; MAX_TASKS],
    process_stacks: [ProcessStack; MAX_TASKS],
    kernel_stack: KernelStack,
    current_process: usize,
    next_pid: usize,
}

impl ProcessManager {
    /// Create a new process manager
    pub const fn new() -> Self {
        Self {
            processes: [ProcessControlBlock {
                stack_pointer: 0,
                process_id: 0,
                state: ProcessState::Ready,
            }; MAX_TASKS],
            process_stacks: [ProcessStack([0; TASK_STACK_SIZE_WORDS]); MAX_TASKS],
            kernel_stack: KernelStack([0; KERNEL_STACK_SIZE_WORDS]),
            current_process: 0,
            next_pid: 0,
        }
    }

    /// Initialize a process with entry point
    pub unsafe fn create_process(&mut self, entry_point: usize) -> Result<usize, &'static str> {
        if self.next_pid >= MAX_TASKS {
            return Err("Maximum processes reached");
        }

        let pid = self.next_pid;
        self.next_pid += 1;

        // Initialize process stack
        let stack_pointer = unsafe {
            ProcessManager::build_initial_stack_static(&mut self.process_stacks[pid].0, entry_point)
        };

        self.processes[pid] = ProcessControlBlock {
            stack_pointer,
            process_id: pid,
            state: ProcessState::Ready,
        };

        rprintln!("[PROCESS] Created process {} at entry 0x{:08X}", pid, entry_point);

        Ok(pid)
    }

    /// Build initial stack frame for a process
    unsafe fn build_initial_stack_static(stack: &mut [u32], entry_point: usize) -> u32 {
        let len = stack.len();
        let base = len - (SW_CONTEXT_SIZE + HW_CONTEXT_SIZE + STACK_GUARD_SIZE);

        // Clear software context (r4-r11)
        for i in 0..SW_CONTEXT_SIZE {
            stack[base + i] = 0;
        }

        // Initialize hardware context
        let hw_base = base + SW_CONTEXT_SIZE;
        stack[hw_base + 0] = 0; // R0
        stack[hw_base + 1] = 0; // R1
        stack[hw_base + 2] = 0; // R2
        stack[hw_base + 3] = 0; // R3
        stack[hw_base + 4] = 0; // R12
        stack[hw_base + 5] = (ProcessManager::task_return_handler as u32) | 1; // LR
        stack[hw_base + 6] = (entry_point as u32) | 1; // PC with Thumb bit
        stack[hw_base + 7] = 0x0100_0000; // xPSR with Thumb state

        let psp = unsafe { stack.as_ptr().add(base) as u32 };

        // Validate stack pointer
        if psp < SRAM_START || psp >= SRAM_END {
            panic!("Invalid stack pointer: 0x{:08X}", psp);
        }

        if psp & 7 != 0 {
            panic!("Stack pointer not aligned: 0x{:08X}", psp);
        }

        psp
    }

    /// Get current process
    pub fn current_process(&self) -> usize {
        self.current_process
    }

    /// Set current process
    pub fn set_current_process(&mut self, pid: usize) {
        if pid < MAX_TASKS {
            self.current_process = pid;
        }
    }

    /// Get process by ID
    pub fn get_process(&self, pid: usize) -> Option<&ProcessControlBlock> {
        if pid < MAX_TASKS {
            Some(&self.processes[pid])
        } else {
            None
        }
    }

    /// Get mutable process by ID
    pub fn get_process_mut(&mut self, pid: usize) -> Option<&mut ProcessControlBlock> {
        if pid < MAX_TASKS {
            Some(&mut self.processes[pid])
        } else {
            None
        }
    }

    /// Get process count
    pub fn process_count(&self) -> usize {
        self.next_pid
    }

    /// Task return handler
    extern "C" fn task_return_handler() -> ! {
        rprintln!("[PROCESS] Task returned, triggering scheduler");
        trigger_pendsv();
        loop {
            cortex_m::asm::wfi();
        }
    }
}

// Global process manager
static mut PROCESS_MANAGER: ProcessManager = ProcessManager::new();

/// Get global process manager
pub fn process_manager() -> &'static mut ProcessManager {
    unsafe { &mut *core::ptr::addr_of_mut!(PROCESS_MANAGER) }
}

// ===== Scheduler =====

/// Scheduler state
pub struct Scheduler {
    initialized: bool,
}

impl Scheduler {
    /// Create new scheduler
    pub const fn new() -> Self {
        Self { initialized: false }
    }

    /// Initialize the scheduler
    pub fn initialize(&mut self) {
        if self.initialized {
            return;
        }

        rprintln!("[SCHEDULER] Initializing scheduler");
        self.initialized = true;
    }

    /// Start the scheduler
    pub fn start(&mut self) -> ! {
        if !self.initialized {
            panic!("Scheduler not initialized");
        }

        rprintln!("[SCHEDULER] Starting scheduler");

        // Initialize SysTick for preemptive scheduling
        unsafe {
            initialize_systick();
        }

        // Trigger first context switch
        trigger_pendsv();

        // Kernel idle loop
        rprintln!("[SCHEDULER] Entering idle loop");
        loop {
            cortex_m::asm::wfi();
        }
    }

    /// Get next process to run (round-robin)
    fn get_next_process(&self, current: usize) -> usize {
        let process_mgr = process_manager();
        let process_count = process_mgr.process_count();

        if process_count == 0 {
            return 0;
        }

        // Simple round-robin: next process in sequence
        let mut next = (current + 1) % process_count;

        // Find next ready process
        for _ in 0..process_count {
            if let Some(process) = process_mgr.get_process(next) {
                if process.state == ProcessState::Ready || process.state == ProcessState::Running {
                    return next;
                }
            }
            next = (next + 1) % process_count;
        }

        // If no ready process found, stay with current
        current
    }
}

// Global scheduler
static mut SCHEDULER: Scheduler = Scheduler::new();

/// Get global scheduler
pub fn scheduler() -> &'static mut Scheduler {
    unsafe { &mut *core::ptr::addr_of_mut!(SCHEDULER) }
}

// ===== GPIO Functions =====

static mut GPIO_INITIALIZED: bool = false;

/// Initialize GPIO system
pub unsafe fn init_gpio() {
    unsafe {
        if GPIO_INITIALIZED {
            return;
        }

        // Enable GPIO clocks
        let mut rcc_ahb1enr = read_volatile(RCC_AHB1ENR as *const u32);
        rcc_ahb1enr |= (1 << 0) | (1 << 2); // GPIOA and GPIOC
        write_volatile(RCC_AHB1ENR as *mut u32, rcc_ahb1enr);

        // Wait for clock to stabilize
        for _ in 0..128 {
            cortex_m::asm::nop();
        }

        // Setup LED pin (PA5) as output
        setup_gpio_output(GPIOA_BASE, LED_PIN);

        // Setup button pin (PC13) as input with pull-up
        setup_gpio_input_pullup(GPIOC_BASE, BUTTON_PIN);

        GPIO_INITIALIZED = true;
    }
}

/// Configure a GPIO pin as output
unsafe fn setup_gpio_output(port_base: u32, pin: u8) {
    unsafe {
        let pin_pos = pin as u32;
        let pin_mask = 0b11 << (pin_pos * 2);

        // Set mode to output (01)
        let moder_addr = (port_base + GPIO_MODER_OFFSET) as *mut u32;
        let mut moder = read_volatile(moder_addr);
        moder &= !pin_mask;
        moder |= 0b01 << (pin_pos * 2);
        write_volatile(moder_addr, moder);

        // Set output type to push-pull (0)
        let otyper_addr = (port_base + GPIO_OTYPER_OFFSET) as *mut u32;
        let mut otyper = read_volatile(otyper_addr);
        otyper &= !(1 << pin_pos);
        write_volatile(otyper_addr, otyper);

        // Set pull-up/pull-down to none (00)
        let pupdr_addr = (port_base + GPIO_PUPDR_OFFSET) as *mut u32;
        let mut pupdr = read_volatile(pupdr_addr);
        pupdr &= !pin_mask;
        write_volatile(pupdr_addr, pupdr);
    }
}

/// Configure a GPIO pin as input with pull-up
unsafe fn setup_gpio_input_pullup(port_base: u32, pin: u8) {
    unsafe {
        let pin_pos = pin as u32;
        let pin_mask = 0b11 << (pin_pos * 2);

        // Set mode to input (00)
        let moder_addr = (port_base + GPIO_MODER_OFFSET) as *mut u32;
        let mut moder = read_volatile(moder_addr);
        moder &= !pin_mask;
        write_volatile(moder_addr, moder);

        // Set pull-up (01)
        let pupdr_addr = (port_base + GPIO_PUPDR_OFFSET) as *mut u32;
        let mut pupdr = read_volatile(pupdr_addr);
        pupdr &= !pin_mask;
        pupdr |= 0b01 << (pin_pos * 2);
        write_volatile(pupdr_addr, pupdr);
    }
}

/// Write to a GPIO pin
pub fn gpio_write(high: bool) {
    let bsrr_addr = (GPIOA_BASE + GPIO_BSRR_OFFSET) as *mut u32;
    let value = if high {
        1u32 << LED_PIN
    } else {
        1u32 << (LED_PIN + 16)
    };

    unsafe {
        write_volatile(bsrr_addr, value);
    }
}

/// Toggle a GPIO pin
pub fn gpio_toggle() {
    unsafe {
        let odr_addr = (GPIOA_BASE + GPIO_ODR_OFFSET) as *mut u32;
        let current = read_volatile(odr_addr);
        let is_high = (current >> LED_PIN) & 1 == 1;
        gpio_write(!is_high);
    }
}

// ===== Hardware Abstraction =====

/// Initialize SysTick
unsafe fn initialize_systick() {
    rprintln!("[CORTEX-M] Initializing SysTick");

    unsafe {
        // Set interrupt priorities
        let mut shpr3 = read_volatile(SCB_SHPR3 as *const u32);
        shpr3 &= !0xFFFF_0000;
        shpr3 |= (PENDSV_PRIORITY as u32) << 24 | (SYSTICK_PRIORITY as u32) << 16;
        write_volatile(SCB_SHPR3 as *mut u32, shpr3);

        // Configure SysTick timer
        write_volatile(SYSTICK_RVR as *mut u32, SYSTICK_RELOAD_10MS);
        write_volatile(SYSTICK_CVR as *mut u32, 0);
        write_volatile(SYSTICK_CSR as *mut u32, (1 << 2) | (1 << 1) | 1);
    }

    rprintln!("[CORTEX-M] SysTick enabled with {}ms period", TIME_SLICE_MS);
}

/// Trigger PendSV for context switching
pub fn trigger_pendsv() {
    unsafe {
        write_volatile(NVIC_ICSR as *mut u32, 1 << 28);
    }
}

/// PendSV context switch handler
#[unsafe(no_mangle)]
pub extern "C" fn pendsv_switch_handler(old_psp: u32) -> u32 {
    let process_mgr = process_manager();
    let current = process_mgr.current_process();

    if old_psp != 0 {
        // Normal context switch - save current process state
        if let Some(process) = process_mgr.get_process_mut(current) {
            process.stack_pointer = old_psp;
            process.state = ProcessState::Ready;
        }

        // Get next process to run
        let sched = scheduler();
        let next = sched.get_next_process(current);

        if next != current {
            process_mgr.set_current_process(next);
            rprintln!("[SCHEDULER] Switch: {} -> {}", current, next);
        }
    } else {
        // First context switch
        rprintln!("[SCHEDULER] First context switch to process {}", current);
    }

    // Get new process stack pointer
    let new_process_id = process_mgr.current_process();
    let new_sp = if let Some(process) = process_mgr.get_process_mut(new_process_id) {
        process.state = ProcessState::Running;
        process.stack_pointer
    } else {
        panic!("Invalid process ID: {}", new_process_id);
    };

    // Validate stack pointer
    if new_sp < SRAM_START || new_sp >= SRAM_END {
        panic!("Invalid stack pointer: 0x{:08X}", new_sp);
    }

    if new_sp & 7 != 0 {
        panic!("Unaligned stack pointer: 0x{:08X}", new_sp);
    }

    new_sp
}

global_asm!(
    r#"
    .global PendSV
    .type PendSV, %function
PendSV:
    mrs     r0, psp
    cbz     r0, first_switch

normal_switch:
    stmdb   r0!, {{r4-r11}}
    bl      {switch_fn}
    ldmia   r0!, {{r4-r11}}
    msr     psp, r0
    bx      lr

first_switch:
    bl      {switch_fn}
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
    switch_fn = sym pendsv_switch_handler
);

/// SysTick interrupt handler
#[cortex_m_rt::exception]
fn SysTick() {
    trigger_pendsv();
}

// ===== Tasks =====

/// Task 0: LED control task
#[unsafe(no_mangle)]
pub extern "C" fn task0_entry() -> ! {
    rprintln!("[TASK0] Starting LED control task");

    // Turn on LED immediately
    gpio_write(true);
    rprintln!("[TASK0] LED should be ON");

    // Cooperative multitasking - work in rounds
    for round in 0..10 {
        rprintln!("[TASK0] LED task round {}", round);

        // Do work for about 1 second
        for _ in 0..CYCLES_PER_MS * 1000 {
            cortex_m::asm::nop();
        }

        // Yield to next task
        rprintln!("[TASK0] Yielding to next task");
        trigger_pendsv();

        // Small delay for context switch
        for _ in 0..1000 {
            cortex_m::asm::nop();
        }
    }

    rprintln!("[TASK0] Completed work, idling");
    loop {
        cortex_m::asm::wfi();
    }
}

/// Task 1: General purpose task
#[unsafe(no_mangle)]
pub extern "C" fn task1_entry() -> ! {
    rprintln!("[TASK1] Starting general purpose task");

    // Cooperative multitasking - work in rounds
    for round in 0..8 {
        rprintln!("[TASK1] General task round {}", round);

        // Toggle LED to show activity
        gpio_toggle();
        rprintln!("[TASK1] LED toggled");

        // Do work for about 1 second
        for _ in 0..CYCLES_PER_MS * 1000 {
            cortex_m::asm::nop();
        }

        // Yield to next task
        rprintln!("[TASK1] Yielding to next task");
        trigger_pendsv();

        // Small delay for context switch
        for _ in 0..1000 {
            cortex_m::asm::nop();
        }
    }

    rprintln!("[TASK1] Completed work, idling");
    loop {
        cortex_m::asm::wfi();
    }
}

/// Task 2: Background task
#[unsafe(no_mangle)]
pub extern "C" fn task2_entry() -> ! {
    rprintln!("[TASK2] Starting background task");

    // Cooperative multitasking - work in rounds
    for round in 0..6 {
        rprintln!("[TASK2] Background task round {}", round);

        // Turn LED off to show different behavior
        gpio_write(false);
        rprintln!("[TASK2] LED turned OFF");

        // Do work for about 0.5 second
        for _ in 0..CYCLES_PER_MS * 500 {
            cortex_m::asm::nop();
        }

        // Turn LED back on
        gpio_write(true);
        rprintln!("[TASK2] LED turned ON");

        // Do work for another 0.5 second
        for _ in 0..CYCLES_PER_MS * 500 {
            cortex_m::asm::nop();
        }

        // Yield to next task
        rprintln!("[TASK2] Yielding to next task");
        trigger_pendsv();

        // Small delay for context switch
        for _ in 0..1000 {
            cortex_m::asm::nop();
        }
    }

    rprintln!("[TASK2] Completed work, idling");
    loop {
        cortex_m::asm::wfi();
    }
}

/// Main entry point
#[entry]
fn main() -> ! {
    // Initialize RTT for debugging
    rtt_init_print!();
    rprintln!("[MINI-OS] Booting...");

    // Initialize GPIO system
    unsafe {
        init_gpio();
        rprintln!("[MINI-OS] GPIO initialized");
    }

    // Initialize process manager and create processes
    let process_mgr = process_manager();

    unsafe {
        // Create processes
        process_mgr.create_process(task0_entry as usize)
            .expect("Failed to create task 0");
        process_mgr.create_process(task1_entry as usize)
            .expect("Failed to create task 1");
        process_mgr.create_process(task2_entry as usize)
            .expect("Failed to create task 2");

        rprintln!("[MINI-OS] Created {} processes", process_mgr.process_count());
    }

    // Initialize and start scheduler
    let scheduler = scheduler();
    scheduler.initialize();

    rprintln!("[MINI-OS] Starting scheduler...");
    scheduler.start();
}