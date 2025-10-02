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
use cortex_m_rt::exception;

// ===== Configuration =====

/// System clock frequency (16 MHz HSI)
pub const SYSTEM_CLOCK_HZ: u32 = 16_000_000;

/// Maximum number of tasks
pub const MAX_TASKS: usize = 3;

/// Task stack size in words (256 bytes per task)
pub const TASK_STACK_SIZE_WORDS: usize = 64;

/// Kernel stack size in words (256 bytes)
pub const KERNEL_STACK_SIZE_WORDS: usize = 64;

/// Software context size (r4-r11, LR)
pub const SW_CONTEXT_SIZE: usize = 9;

/// Hardware context size (r0-r3, r12, lr, pc, xpsr)
pub const HW_CONTEXT_SIZE: usize = 8;

/// Total context size without FPU
pub const TOTAL_CONTEXT_SIZE: usize = SW_CONTEXT_SIZE + HW_CONTEXT_SIZE;

/// Stack guard size (in words)
pub const STACK_GUARD_SIZE: usize = 4;

/// Stack guard pattern for overflow detection
pub const STACK_GUARD_PATTERN: u32 = 0xDEADBEEF;

/// CPU cycles per millisecond
pub const CYCLES_PER_MS: u32 = SYSTEM_CLOCK_HZ / 1000;

/// Memory layout
pub const SRAM_START: u32 = 0x2000_0000;
pub const SRAM_SIZE: u32 = 128 * 1024; // 128KB
pub const SRAM_END: u32 = SRAM_START + SRAM_SIZE;

/// Dedicated memory regions for MSP/PSP separation
/// Memory layout: [Kernel Stack (MSP)] [OS Data] [Task1 PSP] [Task2 PSP] [Task3 PSP]
pub const KERNEL_STACK_SIZE_BYTES: u32 = KERNEL_STACK_SIZE_WORDS as u32 * 4;
pub const TASK_STACK_SIZE_BYTES: u32 = TASK_STACK_SIZE_WORDS as u32 * 4;

/// Kernel stack region (MSP) - starts at SRAM_START
pub const KERNEL_STACK_START: u32 = SRAM_START;
pub const KERNEL_STACK_END: u32 = KERNEL_STACK_START + KERNEL_STACK_SIZE_BYTES;

/// OS data region (ProcessManager, etc.)
pub const OS_DATA_START: u32 = KERNEL_STACK_END;
pub const OS_DATA_SIZE: u32 = 2048; // 2KB for OS data structures
pub const OS_DATA_END: u32 = OS_DATA_START + OS_DATA_SIZE;

/// Application stack region (PSP) - each task gets its own region
pub const APP_STACK_START: u32 = OS_DATA_END;
pub const TOTAL_APP_STACK_SIZE: u32 = MAX_TASKS as u32 * TASK_STACK_SIZE_BYTES;
pub const APP_STACK_END: u32 = APP_STACK_START + TOTAL_APP_STACK_SIZE;

/// Interrupt priorities
pub const PENDSV_PRIORITY: u8 = 0xFF; // Lowest priority
pub const SYSTICK_PRIORITY: u8 = 0xFE;

/// SysTick reload value for 10ms period
pub const SYSTICK_RELOAD_10MS: u32 = (SYSTEM_CLOCK_HZ / 100) - 1;

/// Time slice per task in milliseconds
pub const TIME_SLICE_MS: u32 = 10;

// ===== EXC_RETURN Values =====

/// EXC_RETURN for Thread mode, PSP, no FPU context
pub const EXC_RETURN_THREAD_PSP: u32 = 0xFFFFFFFD;

/// EXC_RETURN for Thread mode, MSP
pub const EXC_RETURN_THREAD_MSP: u32 = 0xFFFFFFF9;

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
    /// Process stack pointer (points to top of saved context)
    pub stack_pointer: u32,
    /// Process ID
    pub process_id: usize,
    /// Process state
    pub state: ProcessState,
    /// EXC_RETURN value for this task
    pub exc_return: u32,
    /// Task priority (for future scheduling enhancement)
    pub priority: u8,
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

/// Comprehensive OS state for pure preemptive multitasking
#[derive(Copy, Clone)]
pub struct OSState {
    /// Currently running task ID
    pub current_task: usize,
    /// Task execution time in SysTick cycles (10ms each)
    pub task_execution_time: [u32; MAX_TASKS],
    /// Number of times each task has been scheduled
    pub task_switch_count: [u32; MAX_TASKS],
    /// Total number of context switches since boot
    pub total_context_switches: u32,
    /// System uptime in SysTick cycles (10ms each)
    pub system_uptime: u32,
    /// Current time slice remaining for current task (in SysTick cycles)
    pub time_slice_remaining: u32,
    /// Task ready queue (bitmask - bit N set = task N is ready)
    pub ready_tasks: u32,
    /// Number of active tasks
    pub active_task_count: usize,
}

impl OSState {
    /// Create new OS state
    pub const fn new() -> Self {
        Self {
            current_task: 0,
            task_execution_time: [0; MAX_TASKS],
            task_switch_count: [0; MAX_TASKS],
            total_context_switches: 0,
            system_uptime: 0,
            time_slice_remaining: 1, // Start with 1 SysTick cycle (10ms)
            ready_tasks: 0,
            active_task_count: 0,
        }
    }

    /// Mark task as ready
    pub fn mark_task_ready(&mut self, task_id: usize) {
        if task_id < MAX_TASKS {
            self.ready_tasks |= 1 << task_id;
        }
    }

    /// Mark task as not ready
    pub fn mark_task_not_ready(&mut self, task_id: usize) {
        if task_id < MAX_TASKS {
            self.ready_tasks &= !(1 << task_id);
        }
    }

    /// Check if task is ready
    pub fn is_task_ready(&self, task_id: usize) -> bool {
        if task_id < MAX_TASKS {
            (self.ready_tasks & (1 << task_id)) != 0
        } else {
            false
        }
    }

    /// Get next ready task using round-robin
    pub fn get_next_ready_task(&self) -> Option<usize> {
        if self.ready_tasks == 0 {
            return None; // No ready tasks
        }

        // Start searching from the task after current_task for round-robin
        let start_task = (self.current_task + 1) % self.active_task_count;

        // Search from start_task to end
        for i in start_task..self.active_task_count {
            if self.is_task_ready(i) {
                return Some(i);
            }
        }

        // Search from 0 to start_task (wrap around)
        for i in 0..start_task {
            if self.is_task_ready(i) {
                return Some(i);
            }
        }

        None
    }
}

/// Process manager with OS state
pub struct ProcessManager {
    processes: [ProcessControlBlock; MAX_TASKS],
    process_stacks: [ProcessStack; MAX_TASKS],
    kernel_stack: KernelStack,
    os_state: OSState,
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
                exc_return: EXC_RETURN_THREAD_PSP, // Thread mode, PSP, no FPU
                priority: 0,
            }; MAX_TASKS],
            process_stacks: [ProcessStack([0; TASK_STACK_SIZE_WORDS]); MAX_TASKS],
            kernel_stack: KernelStack([0; KERNEL_STACK_SIZE_WORDS]),
            os_state: OSState::new(),
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

        // Initialize process stack - each process gets its own unique stack
        rprintln!("[PROCESS] Initializing stack for process {} (stack array index {})", pid, pid);
        let stack_pointer = unsafe {
            ProcessManager::build_initial_stack_static(&mut self.process_stacks[pid].0, entry_point)?
        };
        rprintln!("[PROCESS] Process {} stack initialized with PSP: 0x{:08X}", pid, stack_pointer);

        rprintln!("[DEBUG] Creating PCB for process {} at index {}", pid, pid);

        self.processes[pid] = ProcessControlBlock {
            stack_pointer,
            process_id: pid,
            state: ProcessState::Ready,
            exc_return: EXC_RETURN_THREAD_PSP, // Thread mode, PSP, no FPU
            priority: 0,
        };

        // Update OS state for new task
        self.os_state.mark_task_ready(pid);
        self.os_state.active_task_count += 1;

        rprintln!("[PCB] Process {} control block created", pid);
        rprintln!("[OS] Task {} marked ready, active tasks: {}", pid, self.os_state.active_task_count);
        rprintln!("[PROCESS] Process {} creation complete", pid);

        Ok(pid)
    }

    /// Build initial stack frame for a process following ARM Cortex-M standard
    ///
    /// ARM Cortex-M Stack Layout (HIGH ADDRESS → LOW ADDRESS, stack grows down):
    /// ┌─────────────────┐ ← Stack Top (highest address)
    /// │  Stack Guard    │ ← STACK_GUARD_PATTERN for overflow detection
    /// ├─────────────────┤
    /// │ Hardware Context│ ← CPU automatically saves these on exception entry
    /// │  (8 registers)  │   xPSR, PC, LR, R12, R3, R2, R1, R0 (in this order)
    /// ├─────────────────┤ ← initial_psp points here (for first task start)
    /// │ Software Context│ ← PendSV manually saves these (callee-saved registers)
    /// │  (9 registers)  │   LR, R11, R10, R9, R8, R7, R6, R5, R4 (in this order)
    /// ├─────────────────┤
    /// │  FPU Context    │ ← PendSV saves FPU registers (S16-S31)
    /// │ (16 registers)  │   S31, S30, S29, ..., S17, S16 (vstmdb order)
    /// └─────────────────┘ ← task_psp points here (after full context save)
    ///
    /// Note: FPSCR is removed from manual save (handled by hardware or not needed)
    ///
    unsafe fn build_initial_stack_static(stack: &mut [u32], entry_point: usize) -> Result<u32, &'static str> {
        let len = stack.len();
        let _stack_addr = stack.as_ptr() as u32;

        rprintln!("[STACK] Building stack for entry 0x{:08X}", entry_point);

        // Calculate stack layout with proper 8-byte alignment
        let total_context = SW_CONTEXT_SIZE + HW_CONTEXT_SIZE + STACK_GUARD_SIZE;

        // Ensure 8-byte alignment - ARM Cortex-M requirement
        // Since each word is 4 bytes, we need even number of words for 8-byte alignment
        let aligned_context = (total_context + 1) & !1; // Round up to even number of words
        let stack_base = len - aligned_context;

        // Additional check: ensure the hardware context starts at 8-byte boundary
        // Stack layout (from high to low): Guard | Hardware | Software
        let hw_base_tentative = stack_base + SW_CONTEXT_SIZE;
        let stack_start_addr = stack.as_ptr() as u32;
        let hw_psp_tentative = stack_start_addr + (hw_base_tentative as u32 * 4);

        // If hardware context PSP is not 8-byte aligned, adjust stack_base
        let alignment_offset = if hw_psp_tentative & 7 != 0 { 1 } else { 0 };
        let stack_base = stack_base - alignment_offset;

        // Remove verbose logging

        // 1. Initialize Stack Guard (at the very top - highest addresses)
        let guard_base = stack_base + SW_CONTEXT_SIZE + HW_CONTEXT_SIZE;
        for i in 0..STACK_GUARD_SIZE {
            stack[guard_base + i] = STACK_GUARD_PATTERN;
        }
        // 2. Initialize Hardware Context (CPU automatically saves these)
        let hw_base = stack_base + SW_CONTEXT_SIZE;
        stack[hw_base + 0] = 0x00000000; // R0 - first argument
        stack[hw_base + 1] = 0x01010101; // R1 - second argument
        stack[hw_base + 2] = 0x02020202; // R2 - third argument
        stack[hw_base + 3] = 0x03030303; // R3 - fourth argument
        stack[hw_base + 4] = 0x12121212; // R12 - intra-procedure call register
        stack[hw_base + 5] = 0xFFFFFFFE; // LR - return address (indicates first-time task)
        stack[hw_base + 6] = (entry_point as u32) | 1; // PC - task entry point with Thumb bit
        stack[hw_base + 7] = 0x01000000; // xPSR - Thumb state bit set, no other flags

        rprintln!("[STACK] Hardware context initialized");

        // 3. Initialize Software Context (PendSV saves these callee-saved registers)
        // Order: R4, R5, R6, R7, R8, R9, R10, R11, LR
        let sw_base = stack_base;
        stack[sw_base + 0] = 0x04040404; // R4 - callee-saved register
        stack[sw_base + 1] = 0x05050505; // R5 - callee-saved register
        stack[sw_base + 2] = 0x06060606; // R6 - callee-saved register
        stack[sw_base + 3] = 0x07070707; // R7 - callee-saved register
        stack[sw_base + 4] = 0x08080808; // R8 - callee-saved register
        stack[sw_base + 5] = 0x09090909; // R9 - callee-saved register
        stack[sw_base + 6] = 0x10101010; // R10 - callee-saved register
        stack[sw_base + 7] = 0x11111111; // R11 - callee-saved register
        stack[sw_base + 8] = (ProcessManager::task_return_handler as u32) | 1; // LR - link register with Thumb bit

        rprintln!("[STACK] Software context initialized");

        // 4. Calculate initial PSP for first task start
        // For first task: PSP should point to Hardware Context (CPU will pop this)
        let stack_start_addr = stack.as_ptr() as u32;
        let initial_psp = stack_start_addr + (hw_base as u32 * 4); // Convert word index to byte address

        // 5. Comprehensive validation
        rprintln!("[STACK] Starting PSP validation...");
        unsafe { Self::validate_stack_pointer(initial_psp, "initial stack setup")?; }
        rprintln!("[STACK] PSP validation passed");

        // Ensure the hardware context is 8-byte aligned (ARM requirement)
        rprintln!("[STACK] Checking 8-byte alignment...");
        if initial_psp & 7 != 0 {
            rprintln!("[ERROR] PSP 0x{:08X} not 8-byte aligned", initial_psp);
            return Err("Hardware context not 8-byte aligned");
        }
        rprintln!("[STACK] 8-byte alignment check passed");

        rprintln!("[STACK] Stack initialization completed for 0x{:08X}", entry_point);

        Ok(initial_psp)
    }

    /// Validate stack pointer with comprehensive checks
    unsafe fn validate_stack_pointer(psp: u32, _context: &str) -> Result<(), &'static str> {
        // Simplified validation with minimal logging to avoid RTT buffer issues

        // Check if within SRAM bounds
        if psp < SRAM_START || psp >= SRAM_END {
            rprintln!("[ERROR] PSP 0x{:08X} out of SRAM bounds", psp);
            return Err("Stack pointer out of bounds");
        }

        // Ensure 8-byte alignment (ARM AAPCS requirement)
        if psp & 7 != 0 {
            rprintln!("[ERROR] PSP 0x{:08X} not aligned", psp);
            return Err("Stack pointer not aligned");
        }

        // Check minimum stack space (avoid stack overflow) - reduced for small embedded system
        if psp < SRAM_START + 64 {
            rprintln!("[ERROR] PSP 0x{:08X} too low", psp);
            return Err("Stack overflow risk");
        }

        rprintln!("[VALIDATE] PSP 0x{:08X} OK", psp);
        Ok(())
    }

    /// Check stack guard pattern for overflow detection
    pub unsafe fn check_stack_guards(&self) -> Result<(), &'static str> {
        rprintln!("[GUARD] Checking stack guards for {} processes", self.next_pid);

        for pid in 0..self.next_pid {
            let stack = &self.process_stacks[pid].0;
            let len = stack.len();

            // Calculate guard position using same logic as stack initialization
            let total_context = SW_CONTEXT_SIZE + HW_CONTEXT_SIZE + STACK_GUARD_SIZE;
            let aligned_context = (total_context + 1) & !1;
            let stack_base = len - aligned_context;
            let guard_start = stack_base + SW_CONTEXT_SIZE + HW_CONTEXT_SIZE;

            rprintln!("[GUARD] Checking process {} guard at indices {}-{}",
                      pid, guard_start, guard_start + STACK_GUARD_SIZE - 1);

            for i in 0..STACK_GUARD_SIZE {
                let guard_value = stack[guard_start + i];
                if guard_value != STACK_GUARD_PATTERN {
                    rprintln!("[ERROR] Stack guard corrupted for process {} at index {}: expected 0x{:08X}, got 0x{:08X}",
                              pid, guard_start + i, STACK_GUARD_PATTERN, guard_value);
                    return Err("Stack guard corrupted");
                }
            }
            rprintln!("[GUARD] ✓ Process {} stack guard intact", pid);
        }

        rprintln!("[GUARD] ✓ All stack guards intact");
        Ok(())
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

    /// Get current running task ID
    pub fn current_task(&self) -> usize {
        self.os_state.current_task
    }

    /// Get OS state for debugging
    pub fn get_os_state(&self) -> &OSState {
        &self.os_state
    }

    /// Get mutable OS state
    pub fn get_os_state_mut(&mut self) -> &mut OSState {
        &mut self.os_state
    }

    /// Perform pure preemptive context switch (called by SysTick)
    pub fn preemptive_schedule(&mut self) -> Option<(u32, u32)> {
        // Update system uptime
        self.os_state.system_uptime += 1;

        // Decrement time slice for current task
        if self.os_state.time_slice_remaining > 0 {
            self.os_state.time_slice_remaining -= 1;
        }

        // If time slice expired or no more time, find next task
        if self.os_state.time_slice_remaining == 0 {
            if let Some(next_task) = self.os_state.get_next_ready_task() {
                let current_task = self.os_state.current_task;

                // Only switch if different task
                if next_task != current_task {
                    // Update statistics
                    self.os_state.total_context_switches += 1;
                    self.os_state.task_switch_count[next_task] += 1;

                    // Update current task
                    self.os_state.current_task = next_task;
                    self.os_state.time_slice_remaining = 1; // Reset to 1 tick (10ms)

                    // Update task states
                    self.processes[current_task].state = ProcessState::Ready;
                    self.processes[next_task].state = ProcessState::Running;

                    rprintln!("[SCHED] Switch: {} -> {}, switches: {}",
                             current_task, next_task, self.os_state.total_context_switches);

                    // Return (current_psp, next_psp) for context switch
                    return Some((
                        self.processes[current_task].stack_pointer,
                        self.processes[next_task].stack_pointer
                    ));
                }
            }

            // No task switch needed, reset time slice
            self.os_state.time_slice_remaining = 1;
        }

        // Update execution time for current task
        self.os_state.task_execution_time[self.os_state.current_task] += 1;

        None // No context switch needed
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
        rprintln!("[SCHEDULER] Scheduler initialization complete");
    }

    /// Start the scheduler
    pub fn start(&mut self) -> ! {
        if !self.initialized {
            panic!("Scheduler not initialized");
        }

        rprintln!("[SCHEDULER] Starting scheduler");
        rprintln!("[SCHEDULER] About to initialize SysTick...");

        // Initialize SysTick for preemptive scheduling
        unsafe {
            initialize_systick();
        }
        rprintln!("[SCHEDULER] SysTick initialization completed");

        // Trigger first context switch
        rprintln!("[SCHEDULER] About to trigger first PendSV...");
        rprintln!("[SCHEDULER] Current PSP before trigger: 0x{:08X}", unsafe {
            let psp: u32;
            core::arch::asm!("mrs {}, psp", out(reg) psp);
            psp
        });
        rprintln!("[SCHEDULER] Triggering PendSV interrupt now...");
        trigger_pendsv();
        rprintln!("[SCHEDULER] PendSV triggered, should not reach this line");
        rprintln!("[SCHEDULER] PendSV triggered, should switch to first task soon...");

        // Brief delay to allow PendSV to execute
        for _ in 0..1000 {
            cortex_m::asm::nop();
        }

        // If we reach here, PendSV didn't execute
        rprintln!("[SCHEDULER] WARNING: Still in kernel after PendSV trigger!");
        rprintln!("[SCHEDULER] Entering idle loop - this should not happen for first switch");
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

/// Initialize MSP/PSP stack separation
pub unsafe fn initialize_msp_psp_separation() {
    unsafe {
        rprintln!("[MSP/PSP] Initializing stack separation");

        // Read current MSP
        let current_msp: u32;
        core::arch::asm!(
            "mrs {}, msp",
            out(reg) current_msp,
            options(nomem, nostack)
        );
        rprintln!("[MSP/PSP] Current MSP (bootloader set): 0x{:08X}", current_msp);

        // Initialize PSP to 0 (will be set per task when switching)
        rprintln!("[MSP/PSP] Setting PSP to 0 (will be set per task)...");
        core::arch::asm!(
            "msr psp, {}",
            in(reg) 0u32,
            options(nomem, nostack)
        );

        // Verify PSP was set
        let current_psp: u32;
        core::arch::asm!(
            "mrs {}, psp",
            out(reg) current_psp,
            options(nomem, nostack)
        );
        rprintln!("[MSP/PSP] PSP initialized to: 0x{:08X}", current_psp);
    }

    // Stay in Handler mode using MSP for kernel operations
    // Tasks will switch to Thread mode using individual PSPs
    rprintln!("[MSP/PSP] Stack separation initialized - MSP for kernel, PSP for tasks");
}

/// Get dedicated stack region for task
pub fn get_task_stack_region(task_id: usize) -> (u32, u32) {
    let stack_start = APP_STACK_START + (task_id as u32 * TASK_STACK_SIZE_BYTES);
    let stack_end = stack_start + TASK_STACK_SIZE_BYTES;
    (stack_start, stack_end)
}

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
    rprintln!("[PENDSV] Triggering PendSV interrupt...");

    // Use cortex-m SCB to trigger PendSV safely
    rprintln!("[PENDSV] Setting PendSV using SCB::set_pendsv()");
    cortex_m::peripheral::SCB::set_pendsv();
    rprintln!("[PENDSV] SCB::set_pendsv() completed");

    rprintln!("[PENDSV] PendSV trigger completed, waiting for interrupt...");
}

/// PendSV context switch handler - called from assembly PendSV handler
///
/// This function handles two scenarios:
/// 1. First task switch: old_psp = 0, returns initial PSP pointing to hardware context
/// 2. Normal task switch: old_psp = saved software context, returns next task's software context
///
/// Stack pointer relationship:
/// - For running task: PSP points to hardware context (CPU auto-save/restore)
/// - For saved task: stored PSP points to software context (manual save/restore)
#[unsafe(no_mangle)]
pub extern "C" fn pendsv_switch_handler(old_psp: u32) -> u32 {
    rprintln!("\\n[PENDSV-HANDLER] === ENTERED PendSV Handler ===");
    rprintln!("[PENDSV-HANDLER] old_psp = 0x{:08X}", old_psp);

    let process_mgr = process_manager();
    let current = process_mgr.current_task();

    rprintln!("[PENDSV-HANDLER] Current process: {}", current);

    if old_psp != 0 {
        // =====================================================================
        // NORMAL TASK-TO-TASK CONTEXT SWITCH
        // =====================================================================
        rprintln!("[SCHEDULER] Normal switch from process {}, saving context at PSP: 0x{:08X}",
                  current, old_psp);

        // Save current task's context
        if let Some(process) = process_mgr.get_process_mut(current) {
            // Store PSP pointing to software context (where R4-R11,LR were saved)
            process.stack_pointer = old_psp;
            process.state = ProcessState::Ready;

            // Validation
            if old_psp < SRAM_START || old_psp >= SRAM_END {
                panic!("Invalid saved PSP: 0x{:08X} for process {}", old_psp, current);
            }
            if old_psp & 7 != 0 {
                panic!("Misaligned saved PSP: 0x{:08X} for process {}", old_psp, current);
            }
        } else {
            panic!("Invalid current process: {}", current);
        }

        // Select next task to run
        let sched = scheduler();
        let next = sched.get_next_process(current);

        if next != current {
            process_mgr.get_os_state_mut().current_task = next;
            rprintln!("[SCHEDULER] Context switch: {} -> {}", current, next);
        } else {
            rprintln!("[SCHEDULER] Continuing process {}", current);
        }

        // Get next task's saved context
        let new_process_id = process_mgr.current_task();
        let new_process = process_mgr.get_process_mut(new_process_id)
            .unwrap_or_else(|| panic!("Invalid next process ID: {}", new_process_id));

        new_process.state = ProcessState::Running;
        let saved_context_psp = new_process.stack_pointer;

        rprintln!("[SCHEDULER] Restoring process {} from saved context at PSP: 0x{:08X}",
                  new_process_id, saved_context_psp);

        // Return PSP pointing to saved software context for restoration
        saved_context_psp

    } else {
        // =====================================================================
        // FIRST TASK SWITCH - KERNEL TO FIRST USER TASK
        // =====================================================================
        rprintln!("[SCHEDULER] First task switch to process {}", current);

        // Get first task
        let first_process = process_mgr.get_process_mut(current)
            .unwrap_or_else(|| panic!("Invalid first process ID: {}", current));

        first_process.state = ProcessState::Running;
        let initial_psp = first_process.stack_pointer;

        rprintln!("[SCHEDULER] Starting first task {} with initial PSP: 0x{:08X}",
                  current, initial_psp);

        // For first task: PSP should point to hardware context (pre-initialized)
        // CPU will restore R0-R3, R12, LR, PC, xPSR from this location
        // No software context restoration needed - task starts fresh

        // Validation
        if initial_psp < SRAM_START || initial_psp >= SRAM_END {
            panic!("Invalid initial PSP for first task: 0x{:08X}", initial_psp);
        }
        if initial_psp & 7 != 0 {
            panic!("Misaligned initial PSP for first task: 0x{:08X}", initial_psp);
        }

        // Return initial PSP pointing to hardware context
        initial_psp
    }
}

global_asm!(
    r#"
    .global PendSV
    .type   PendSV, %function
    .thumb
    .thumb_func
PendSV_Handler:
    /* Cortex-M PendSV_Handler (FreeRTOS-style, no FPU) */

    /* LED ON to indicate PendSV entry */
    ldr     r1, =0x40020018     /* GPIOA_BSRR */
    mov     r2, #32             /* Set bit 5 (LED ON) */
    str     r2, [r1]            /* GIOPA_BSRR 레지스터에 LED_ON 값을 넣어주는 역할 */

    /* Save current PSP into r0 */
    mrs     r0, psp

    /* Check first switch (PSP == 0) */
    /* Compare and Branch on Zero : r0 == 0이면 first_task 레이블로 분기. 첫 태스크 실행 전 PSP가 0일 때 사용. */
    cbz     r0, first_task_switch   

    /* NORMAL TASK SWITCH */
normal_task_switch:
    /* Save callee-saved registers + LR */
    stmdb   r0!, {{r4-r11, r14}}

    /* Call Rust handler */
    bl      {0}     /* cur PC +  4를 LR에 저장 -> 함수 호출처럼 동작 */

    /* Restore callee-saved registers + LR */
    ldmia   r0!, {{r4-r11, r14}}

    /* Update PSP to point to HW context */
    msr     psp, r0

    /* Exception return: CPU will restore HW context */
    bx      lr

    /* FIRST TASK SWITCH */
first_task_switch:
    /* Ask Rust handler for first task PSP */
    bl      {0}

    /* r0 = PSP of first task (points to HW context) */
    msr     psp, r0

    /* Configure CONTROL: use PSP in Thread mode */
    mrs     r1, CONTROL
    orr     r1, r1, #0x02   /* SPSEL: use PSP */
    msr     CONTROL, r1
    isb

    /* Return with EXC_RETURN for Thread+PSP (no FPU) */
    ldr     lr, =0xFFFFFFFD
    bx      lr
"#,
    sym pendsv_switch_handler
);



// ===== Tasks =====

/// Task 0: LED control task (Pure Preemptive)
#[unsafe(no_mangle)]
pub extern "C" fn task0_entry() -> ! {
    rprintln!("[TASK0] Starting pure preemptive LED control task");

    // Enable SysTick now that first task has started
    rprintln!("[TASK0] Enabling SysTick for preemptive scheduling");
    unsafe {
        initialize_systick();
    }
    rprintln!("[TASK0] SysTick enabled - preemptive multitasking active");

    let mut counter = 0u32;

    // Pure preemptive task - runs continuously until preempted by SysTick
    loop {
        // Turn LED on
        gpio_write(true);

        // Do some work - will be preempted by SysTick every 10ms
        counter = counter.wrapping_add(1);

        // Minimal work to show task is running
        for _ in 0..1000 {
            cortex_m::asm::nop();
        }

        // Every 100000 iterations, print status
        if counter % 100000 == 0 {
            rprintln!("[TASK0] LED task running, counter: {}", counter);
        }
    }
}

/// Task 1: General purpose task (Pure Preemptive)
#[unsafe(no_mangle)]
pub extern "C" fn task1_entry() -> ! {
    rprintln!("[TASK1] Starting pure preemptive general task");

    let mut counter = 0u32;

    // Pure preemptive task - runs continuously until preempted by SysTick
    loop {
        // Toggle LED to show activity
        gpio_toggle();

        // Do some work - will be preempted by SysTick every 10ms
        counter = counter.wrapping_add(1);

        // Minimal work to show task is running
        for _ in 0..2000 {
            cortex_m::asm::nop();
        }

        // Every 50000 iterations, print status
        if counter % 50000 == 0 {
            rprintln!("[TASK1] General task running, counter: {}", counter);
        }
    }
}

/// Task 2: Background task (Pure Preemptive)
#[unsafe(no_mangle)]
pub extern "C" fn task2_entry() -> ! {
    rprintln!("[TASK2] Starting pure preemptive background task");

    let mut counter = 0u32;
    let mut led_state = false;

    // Pure preemptive task - runs continuously until preempted by SysTick
    loop {
        // Alternate LED state every few iterations
        counter = counter.wrapping_add(1);

        if counter % 3000 == 0 {
            led_state = !led_state;
            gpio_write(led_state);
        }

        // Minimal work to show task is running
        for _ in 0..500 {
            cortex_m::asm::nop();
        }

        // Every 25000 iterations, print status
        if counter % 25000 == 0 {
            rprintln!("[TASK2] Background task running, counter: {}, LED: {}",
                     counter, if led_state { "ON" } else { "OFF" });
        }
    }
}

// ===== Exception Handlers =====

/// SysTick handler - performs pure preemptive scheduling every 10ms
#[unsafe(no_mangle)]
pub extern "C" fn SysTick() {
    // Get process manager and perform preemptive scheduling
    let process_mgr = process_manager();

    if let Some((current_psp, _next_psp)) = process_mgr.preemptive_schedule() {
        // A context switch is needed - update current task's PSP first
        process_mgr.processes[process_mgr.os_state.current_task].stack_pointer = current_psp;

        // Trigger PendSV for actual context switching
        trigger_pendsv();
    }
    // If no context switch needed, just continue with current task
}


/// Main entry point
#[entry]
fn main() -> ! {
    // Initialize RTT for debugging
    rtt_init_print!();

    // Add small delay for RTT to initialize properly
    for _ in 0..100000 {
        cortex_m::asm::nop();
    }

    rprintln!("[MINI-OS] Booting...");

    // Initialize MSP/PSP stack separation
    unsafe {
        initialize_msp_psp_separation();
    }
    rprintln!("[MAIN] MSP/PSP initialization completed");

    // Initialize GPIO system
    rprintln!("[MAIN] Starting GPIO initialization...");
    unsafe {
        init_gpio();
        rprintln!("[MAIN] GPIO initialization completed");

        // LED ON = GPIO initialized (Step 1)
        gpio_write(true);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED ON - GPIO ready");

        // LED OFF = Starting process manager (Step 2)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Starting ProcessManager");
    }

    // Initialize process manager and create processes
    rprintln!("[MAIN] Creating ProcessManager...");
    let process_mgr = process_manager();
    rprintln!("[MAIN] ProcessManager created successfully");

    // LED ON = Process manager ready (Step 3)
    gpio_write(true);
    for _ in 0..250000 { cortex_m::asm::nop(); }
    rprintln!("[MAIN] LED ON - ProcessManager ready");

    // Memory usage analysis
    let mgr_addr = process_mgr as *const _ as u32;
    let mgr_size = core::mem::size_of::<ProcessManager>();
    rprintln!("[MEM] ProcessManager at 0x{:08X}, size: {} bytes", mgr_addr, mgr_size);
    rprintln!("[MEM] Total task stack memory: {} bytes", MAX_TASKS * TASK_STACK_SIZE_WORDS * 4);
    rprintln!("[MEM] SRAM usage: {}/{} bytes", mgr_addr - SRAM_START + mgr_size as u32, SRAM_SIZE);

    rprintln!("[MAIN] Starting process creation...");
    unsafe {
        // Create processes with LED indicators

        // LED OFF = Starting process 0 creation (Step 4)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Creating Process 0");

        let _pid0 = process_mgr.create_process(task0_entry as usize)
            .expect("Failed to create task 0");
        rprintln!("[MAIN] Process 0 created successfully");

        // LED ON = Process 0 created (Step 5)
        gpio_write(true);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED ON - Process 0 ready");

        // LED OFF = Starting process 1 creation (Step 6)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Creating Process 1");

        process_mgr.create_process(task1_entry as usize)
            .expect("Failed to create task 1");
        rprintln!("[MAIN] Process 1 created successfully");

        // LED ON = Process 1 created (Step 7)
        gpio_write(true);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED ON - Process 1 ready");

        // LED OFF = Starting process 2 creation (Step 8)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Creating Process 2");

        process_mgr.create_process(task2_entry as usize)
            .expect("Failed to create task 2");
        rprintln!("[MAIN] Process 2 created successfully");

        // LED ON = All processes created (Step 9)
        gpio_write(true);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED ON - All processes created");
    }

    rprintln!("[MAIN] All {} processes created successfully!", MAX_TASKS);

    // Initialize pure preemptive scheduler
    rprintln!("[MAIN] Starting preemptive scheduler initialization...");
    unsafe {
        // LED OFF = Starting scheduler initialization (Step 10)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Starting scheduler");

        rprintln!("[PREEMPTIVE] Starting pure preemptive multitasking");

        // Set first task as current and running BEFORE enabling SysTick
        rprintln!("[PREEMPTIVE] Setting up first task state...");
        process_mgr.os_state.current_task = 0;
        process_mgr.processes[0].state = ProcessState::Running;
        rprintln!("[PREEMPTIVE] Task 0 set as initial running task");

        // LED ON = First task ready (Step 11)
        gpio_write(true);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED ON - First task ready");

        // Initialize SysTick AFTER first task setup (but don't enable yet)
        rprintln!("[CORTEX-M] Preparing SysTick (not enabled yet)");
        // Note: We'll enable SysTick after first task starts

        // LED OFF = Preparing PSP (Step 12)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Setting PSP");

        // Set PSP to first task's stack pointer for direct jump
        let initial_psp = process_mgr.processes[0].stack_pointer;
        rprintln!("[PREEMPTIVE] First task PSP (hardware context): 0x{:08X}", initial_psp);

        // Validate PSP range
        if initial_psp < SRAM_START || initial_psp >= SRAM_END {
            rprintln!("[ERROR] PSP 0x{:08X} out of SRAM bounds!", initial_psp);
        } else {
            rprintln!("[PREEMPTIVE] PSP validation: OK");
        }

        // Detailed stack frame verification
        rprintln!("[STACK_VERIFY] Analyzing task 0 stack frame...");
        let stack_base_ptr = initial_psp as *const u32;
        let r0 = core::ptr::read_volatile(stack_base_ptr.offset(0));
        let r1 = core::ptr::read_volatile(stack_base_ptr.offset(1));
        let r2 = core::ptr::read_volatile(stack_base_ptr.offset(2));
        let r3 = core::ptr::read_volatile(stack_base_ptr.offset(3));
        let r12 = core::ptr::read_volatile(stack_base_ptr.offset(4));
        let lr = core::ptr::read_volatile(stack_base_ptr.offset(5));
        let pc = core::ptr::read_volatile(stack_base_ptr.offset(6));
        let xpsr = core::ptr::read_volatile(stack_base_ptr.offset(7));

        rprintln!("[HW_CONTEXT] R0=0x{:08X}, R1=0x{:08X}, R2=0x{:08X}, R3=0x{:08X}", r0, r1, r2, r3);
        rprintln!("[HW_CONTEXT] R12=0x{:08X}, LR=0x{:08X}", r12, lr);
        rprintln!("[HW_CONTEXT] PC=0x{:08X}, xPSR=0x{:08X}", pc, xpsr);

        // Verify PC has Thumb bit set and points to valid task
        if (pc & 1) == 0 {
            rprintln!("[ERROR] PC 0x{:08X} missing Thumb bit!", pc);
        } else {
            rprintln!("[VERIFY] PC Thumb bit check: OK");
        }

        // Verify xPSR has Thumb state bit
        if (xpsr & 0x01000000) == 0 {
            rprintln!("[ERROR] xPSR 0x{:08X} missing Thumb state!", xpsr);
        } else {
            rprintln!("[VERIFY] xPSR Thumb state check: OK");
        }

        // Set PSP register
        rprintln!("[PREEMPTIVE] Setting PSP register...");
        core::arch::asm!(
            "msr psp, {}",
            in(reg) initial_psp
        );

        // Verify PSP was set correctly
        let current_psp: u32;
        core::arch::asm!(
            "mrs {}, psp",
            out(reg) current_psp
        );
        rprintln!("[PREEMPTIVE] PSP set and verified: 0x{:08X}", current_psp);

        // Verify PSP matches what we set
        if current_psp != initial_psp {
            rprintln!("[ERROR] PSP mismatch! Set: 0x{:08X}, Read: 0x{:08X}", initial_psp, current_psp);
        } else {
            rprintln!("[VERIFY] PSP register check: OK");
        }

        // Read and log current CPU state before Thread mode switch
        let current_msp: u32;
        let current_control: u32;
        let current_primask: u32;
        core::arch::asm!(
            "mrs {}, msp",
            out(reg) current_msp
        );
        core::arch::asm!(
            "mrs {}, control",
            out(reg) current_control
        );
        core::arch::asm!(
            "mrs {}, primask",
            out(reg) current_primask
        );

        rprintln!("[CPU_STATE] Before Thread switch:");
        rprintln!("[CPU_STATE] MSP=0x{:08X}, PSP=0x{:08X}", current_msp, current_psp);
        rprintln!("[CPU_STATE] CONTROL=0x{:08X} (SPSEL={}, nPRIV={})",
                 current_control, (current_control >> 1) & 1, current_control & 1);
        rprintln!("[CPU_STATE] PRIMASK=0x{:08X} (interrupts {})",
                 current_primask, if current_primask & 1 != 0 { "disabled" } else { "enabled" });

        // LED ON = PSP ready (Step 13)
        gpio_write(true);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED ON - PSP configured");

        // Final preparation before Thread mode switch
        rprintln!("[PREEMPTIVE] About to switch to Thread mode...");
        rprintln!("[PREEMPTIVE] EXC_RETURN will be: 0xFFFFFFFD (Thread+PSP)");

        // LED OFF = About to switch to Thread mode (Step 14)
        gpio_write(false);
        for _ in 0..250000 { cortex_m::asm::nop(); }
        rprintln!("[MAIN] LED OFF - Switching to Thread mode NOW!");

        // Start first task through PendSV first_task_switch
        rprintln!("[PREEMPTIVE] Starting first task via PendSV first_task_switch");

        // Keep PSP at 0 so PendSV will use first_task_switch path
        // PendSV will set up the proper PSP and switch to first task

        // Enable interrupts and trigger PendSV to start first task
        rprintln!("[PREEMPTIVE] Triggering PendSV for first task startup");
        trigger_pendsv();

        // Wait for PendSV to start the first task
        rprintln!("[PREEMPTIVE] Waiting for first task to start...");

        // Force interrupt enable and check status
        core::arch::asm!("cpsie i"); // Clear PRIMASK - enable interrupts
        core::arch::asm!("msr basepri, {}", in(reg) 0u32); // Clear BASEPRI - allow all priorities

        // Read interrupt status registers
        let primask: u32;
        let basepri: u32;
        let _shpr3: u32;

        core::arch::asm!("mrs {}, primask", out(reg) primask);
        core::arch::asm!("mrs {}, basepri", out(reg) basepri);
        let shpr3 = read_volatile(0xE000ED20u32 as *const u32); // SHPR3

        rprintln!("[INT_STATUS] PRIMASK: 0x{:02X} ({})", primask,
                 if primask & 1 != 0 { "DISABLED" } else { "ENABLED" });
        rprintln!("[INT_STATUS] BASEPRI: 0x{:02X}", basepri);
        rprintln!("[INT_STATUS] SHPR3: 0x{:08X}", shpr3);
        rprintln!("[INT_STATUS] PendSV priority: {}", (shpr3 >> 16) & 0xFF);
        rprintln!("[INT_STATUS] SysTick priority: {}", (shpr3 >> 24) & 0xFF);
        

        // Busy wait instead of WFI to ensure PendSV can execute
        let mut counter = 0u32;
        loop {
            cortex_m::asm::nop();
            counter += 1;
            if counter % 100000 == 0 {
                rprintln!("[WAIT] Waiting for PendSV... counter: {}", counter);
            }
        }
    }
}

/// Hard Fault Handler for debugging
#[exception]
unsafe fn HardFault(_ef: &cortex_m_rt::ExceptionFrame) -> ! {
    rprintln!("[HARDFAULT] Exception occurred!");

    // Read fault status registers
    let cfsr = unsafe { read_volatile(0xE000ED28 as *const u32) }; // Configurable Fault Status Register
    let hfsr = unsafe { read_volatile(0xE000ED2C as *const u32) }; // Hard Fault Status Register
    let mmfar = unsafe { read_volatile(0xE000ED34 as *const u32) }; // MemManage Fault Address Register
    let bfar = unsafe { read_volatile(0xE000ED38 as *const u32) }; // Bus Fault Address Register

    rprintln!("[HARDFAULT] CFSR: 0x{:08X}", cfsr);
    rprintln!("[HARDFAULT] HFSR: 0x{:08X}", hfsr);
    rprintln!("[HARDFAULT] MMFAR: 0x{:08X}", mmfar);
    rprintln!("[HARDFAULT] BFAR: 0x{:08X}", bfar);

    // Current PSP
    let psp: u32;
    unsafe { core::arch::asm!("mrs {}, psp", out(reg) psp) };
    rprintln!("[HARDFAULT] Current PSP: 0x{:08X}", psp);

    // MSP
    let msp: u32;
    unsafe { core::arch::asm!("mrs {}, msp", out(reg) msp) };
    rprintln!("[HARDFAULT] Current MSP: 0x{:08X}", msp);

    rprintln!("[HARDFAULT] System halted");
    loop {
        cortex_m::asm::wfi();
    }
}