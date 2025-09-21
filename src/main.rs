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

/// FPU extended context size (s16-s31 only, FPSCR handled by hardware)
pub const FPU_CONTEXT_SIZE: usize = 16;

/// Total context size with FPU support
pub const TOTAL_CONTEXT_SIZE: usize = SW_CONTEXT_SIZE + HW_CONTEXT_SIZE + FPU_CONTEXT_SIZE;

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

/// EXC_RETURN for Thread mode, PSP, with FPU context
pub const EXC_RETURN_THREAD_PSP_FPU: u32 = 0xFFFFFFED;

// ===== Register Addresses =====

/// NVIC ICSR register
pub const NVIC_ICSR: u32 = 0xE000_ED04;

/// SCB SHPR3 register
pub const SCB_SHPR3: u32 = 0xE000_ED20;

/// FPU registers
pub const FPU_CPACR: u32 = 0xE000_ED88; // Coprocessor Access Control Register
pub const FPU_FPCCR: u32 = 0xE000_EF34; // FP Context Control Register
pub const FPU_FPCAR: u32 = 0xE000_EF38; // FP Context Address Register
pub const FPU_FPDSCR: u32 = 0xE000_EF3C; // FP Default Status Control Register

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
    /// FPU context enabled for this task
    pub fpu_enabled: bool,
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
                exc_return: EXC_RETURN_THREAD_PSP_FPU, // Thread mode, PSP, with FPU
                priority: 0,
                fpu_enabled: true,
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

        // Initialize process stack - each process gets its own unique stack
        rprintln!("[PROCESS] Initializing stack for process {} (stack array index {})", pid, pid);
        let stack_pointer = unsafe {
            ProcessManager::build_initial_stack_static(&mut self.process_stacks[pid].0, entry_point)?
        };
        rprintln!("[PROCESS] Process {} stack initialized with PSP: 0x{:08X}", pid, stack_pointer);
        rprintln!("[DEBUG] Setting PCB for process {} at index {}", pid, pid);

        self.processes[pid] = ProcessControlBlock {
            stack_pointer,
            process_id: pid,
            state: ProcessState::Ready,
            exc_return: EXC_RETURN_THREAD_PSP_FPU, // Thread mode, PSP, with FPU
            priority: 0,
            fpu_enabled: true,
        };

        rprintln!("[PCB] Process {} control block created", pid);
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

        // Calculate stack layout with proper 8-byte alignment including FPU context
        let total_context = SW_CONTEXT_SIZE + HW_CONTEXT_SIZE + FPU_CONTEXT_SIZE + STACK_GUARD_SIZE;

        // Ensure 8-byte alignment - ARM Cortex-M requirement
        // Since each word is 4 bytes, we need even number of words for 8-byte alignment
        let aligned_context = (total_context + 1) & !1; // Round up to even number of words
        let stack_base = len - aligned_context;

        // Additional check: ensure the hardware context starts at 8-byte boundary
        // Stack layout (from high to low): Guard | Hardware | Software | FPU
        let hw_base_tentative = stack_base + SW_CONTEXT_SIZE + FPU_CONTEXT_SIZE;
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
        let hw_base = stack_base + SW_CONTEXT_SIZE + FPU_CONTEXT_SIZE;
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

        // 4. Initialize FPU Extended Context (s16-s31, FPSCR)
        // FPU context goes below Software Context (lowest addresses)

        // Check for underflow before subtracting
        if stack_base < FPU_CONTEXT_SIZE {
            rprintln!("[ERROR] Stack base {} too small for FPU context size {}",
                      stack_base, FPU_CONTEXT_SIZE);
            return Err("Stack too small for FPU context");
        }

        let fpu_base = stack_base - FPU_CONTEXT_SIZE;

        // Validate FPU context indices
        if fpu_base >= len || (fpu_base + FPU_CONTEXT_SIZE) > len {
            rprintln!("[ERROR] FPU context indices out of bounds: fpu_base={}, len={}, FPU_SIZE={}",
                      fpu_base, len, FPU_CONTEXT_SIZE);
            return Err("FPU context out of bounds");
        }

        // Initialize S16-S31 registers (FPSCR handled by hardware)
        for i in 0..16 {
            stack[fpu_base + i] = 0x16160000 + i as u32; // S16-S31 registers
        }

        rprintln!("[STACK] FPU context initialized");

        // 5. Calculate initial PSP for first task start
        // For first task: PSP should point to Hardware Context (CPU will pop this)
        // Note: FPU context is below software context and will be skipped for first task
        let stack_start_addr = stack.as_ptr() as u32;
        let initial_psp = stack_start_addr + (hw_base as u32 * 4); // Convert word index to byte address

        // 5. Comprehensive validation
        unsafe { Self::validate_stack_pointer(initial_psp, "initial stack setup")?; }

        // Ensure the hardware context is 8-byte aligned (ARM requirement)
        if initial_psp & 7 != 0 {
            return Err("Hardware context not 8-byte aligned");
        }

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
            let total_context = SW_CONTEXT_SIZE + HW_CONTEXT_SIZE + FPU_CONTEXT_SIZE + STACK_GUARD_SIZE;
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

// ===== FPU Functions =====

static mut FPU_INITIALIZED: bool = false;

/// Initialize FPU for STM32F446 (Cortex-M4F)
pub unsafe fn init_fpu() {
    unsafe {
        if FPU_INITIALIZED {
            return;
        }

        rprintln!("[FPU] Initializing FPU...");

        // 1. Enable CP10 and CP11 coprocessors (FPU access)
        let mut cpacr = read_volatile(FPU_CPACR as *const u32);
        cpacr |= 0xF << 20; // Full access for CP10 and CP11
        write_volatile(FPU_CPACR as *mut u32, cpacr);

        // 2. Configure FP Context Control Register (FPCCR) - Disable ALL lazy stacking
        let mut fpccr = read_volatile(FPU_FPCCR as *const u32);
        fpccr &= !(1 << 31); // Disable ASPEN (automatic lazy context save)
        fpccr &= !(1 << 30); // Disable LSPEN (lazy state preservation) for deterministic behavior
        write_volatile(FPU_FPCCR as *mut u32, fpccr);

        // 3. Initialize FP Default Status Control Register
        write_volatile(FPU_FPDSCR as *mut u32, 0);

        // 4. Memory barrier to ensure FPU configuration is complete
        cortex_m::asm::dsb();
        cortex_m::asm::isb();

        FPU_INITIALIZED = true;
        rprintln!("[FPU] FPU initialized successfully");
        rprintln!("[FPU] CPACR=0x{:08X}, FPCCR=0x{:08X}",
                  read_volatile(FPU_CPACR as *const u32),
                  read_volatile(FPU_FPCCR as *const u32));
    }
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
    rprintln!("[PENDSV] Triggering PendSV interrupt...");
    unsafe {
        // Set PendSV bit in NVIC ICSR register
        write_volatile(NVIC_ICSR as *mut u32, 1 << 28);

        // Read back to verify
        let icsr = read_volatile(NVIC_ICSR as *const u32);
        rprintln!("[PENDSV] ICSR register after trigger: 0x{:08X}", icsr);
        rprintln!("[PENDSV] PendSV pending bit: {}", (icsr >> 28) & 1);
    }
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
    let current = process_mgr.current_process();

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
            process_mgr.set_current_process(next);
            rprintln!("[SCHEDULER] Context switch: {} -> {}", current, next);
        } else {
            rprintln!("[SCHEDULER] Continuing process {}", current);
        }

        // Get next task's saved context
        let new_process_id = process_mgr.current_process();
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
    .type PendSV, %function
PendSV:
    @ =========================================================================
    @ ARM Cortex-M PendSV Handler for Context Switching (FreeRTOS Style)
    @ =========================================================================
    @
    @ Based on ARM Application Note and FreeRTOS xPortPendSVHandler
    @ Stack Layout: HW Context → Callee-Saved (R4-R11,LR) → FPU Context (S16-S31)
    @ =========================================================================

    @ Disable interrupts at configurable level (FreeRTOS style)
    mov     r0, #0xFF               @ BASEPRI mask value
    msr     basepri, r0             @ Set BASEPRI instead of global disable
    dsb                             @ Data synchronization barrier
    isb                             @ Instruction synchronization barrier

    @ Get current Process Stack Pointer
    mrs     r0, psp

    @ Check if this is the first task switch (PSP == 0)
    cbz     r0, first_task_switch

    @ -------------------------------------------------------------------------
    @ NORMAL TASK SWITCH: Save current task, switch to new task
    @ -------------------------------------------------------------------------
normal_task_switch:
    @ Save FPU extended context first (S16-S31) - Always save for deterministic behavior
    @ FPU lazy stacking is disabled, so we manually save/restore
    vstmdb  r0!, {{s16-s31}}        @ Push S16-S31 to stack (16 regs × 4 bytes = 64 bytes)

    @ Save current task's callee-saved registers (ARM AAPCS standard)
    @ Stack layout after: ... | HW Context | R4-R11,LR | S16-S31 | <- PSP
    stmdb   r0!, {{r4-r11, r14}}   @ Push R4-R11, LR (9 words = 36 bytes)

    @ Call Rust scheduler to select next task
    @ Input:  r0 = current task's PSP (pointing to saved software context)
    @ Output: r0 = next task's PSP (pointing to software context to restore)
    bl      {switch_fn}

    @ Restore next task's callee-saved registers
    @ After this: r0 points to FPU context, LR contains next task's LR
    ldmia   r0!, {{r4-r11, r14}}

    @ Restore FPU extended context (S16-S31)
    vldmia  r0!, {{s16-s31}}

    @ Update PSP to point to hardware context (for CPU auto-restore)
    msr     psp, r0

    @ Clear BASEPRI to enable interrupts
    mov     r0, #0
    msr     basepri, r0
    dsb
    isb

    @ Standard exception return - CPU will restore HW context automatically
    @ EXC_RETURN in LR determines return mode (Thread mode, PSP, FPU context)
    bx      lr

    @ -------------------------------------------------------------------------
    @ FIRST TASK SWITCH: Initialize first task from kernel mode
    @ -------------------------------------------------------------------------
first_task_switch:
    @ Call Rust scheduler to get first task's initial PSP
    @ Input:  r0 = 0 (no current task to save)
    @ Output: r0 = first task's initial PSP (pointing to hardware context)
    bl      {switch_fn}

    @ r0 now contains initial PSP pointing to hardware context
    @ No software context to restore for first task - it's pristine

    @ Set PSP for first task - points to pre-initialized hardware context
    msr     psp, r0

    @ Configure CONTROL register: use PSP in Thread mode, enable FPU
    mrs     r1, CONTROL
    orr     r1, r1, #0x02          @ SPSEL: Use PSP for Thread mode
    orr     r1, r1, #0x04          @ FPCA: FPU context active
    msr     CONTROL, r1
    isb                            @ Instruction Synchronization Barrier

    @ Clear BASEPRI to enable interrupts
    mov     r1, #0
    msr     basepri, r1

    @ Return to first task in Thread mode using PSP
    @ EXC_RETURN = 0xFFFFFFED: Thread mode, PSP, FPU context present
    ldr     lr, =0xFFFFFFED
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

    // Add small delay for RTT to initialize properly
    for _ in 0..100000 {
        cortex_m::asm::nop();
    }

    rprintln!("[MINI-OS] Booting...");

    // Initialize FPU for Cortex-M4F
    unsafe {
        init_fpu();
    }

    // Initialize GPIO system
    unsafe {
        init_gpio();
        // LED ON = GPIO initialized
        gpio_write(true);
        for _ in 0..1000000 { cortex_m::asm::nop(); }

        // LED OFF = Starting process manager
        gpio_write(false);
        for _ in 0..1000000 { cortex_m::asm::nop(); }
    }

    // Initialize process manager and create processes
    let process_mgr = process_manager();

    // LED ON = Process manager ready
    gpio_write(true);
    for _ in 0..1000000 { cortex_m::asm::nop(); }

    // Memory usage analysis
    let mgr_addr = process_mgr as *const _ as u32;
    let mgr_size = core::mem::size_of::<ProcessManager>();
    rprintln!("[MEM] ProcessManager at 0x{:08X}, size: {} bytes", mgr_addr, mgr_size);
    rprintln!("[MEM] Total task stack memory: {} bytes", MAX_TASKS * TASK_STACK_SIZE_WORDS * 4);
    rprintln!("[MEM] SRAM usage: {}/{} bytes", mgr_addr - SRAM_START + mgr_size as u32, SRAM_SIZE);

    unsafe {
        // Create processes with LED indicators

        // LED OFF = Starting process 0
        gpio_write(false);
        for _ in 0..500000 { cortex_m::asm::nop(); }

        let _pid0 = process_mgr.create_process(task0_entry as usize)
            .expect("Failed to create task 0");

        // LED ON = Process 0 created
        gpio_write(true);
        for _ in 0..500000 { cortex_m::asm::nop(); }

        // LED OFF = Starting process 1
        gpio_write(false);
        for _ in 0..500000 { cortex_m::asm::nop(); }

        process_mgr.create_process(task1_entry as usize)
            .expect("Failed to create task 1");

        // LED ON = Process 1 created
        gpio_write(true);
        for _ in 0..500000 { cortex_m::asm::nop(); }

        // LED OFF = Starting process 2
        gpio_write(false);
        for _ in 0..500000 { cortex_m::asm::nop(); }

        process_mgr.create_process(task2_entry as usize)
            .expect("Failed to create task 2");

        // LED ON = All processes created
        gpio_write(true);
        for _ in 0..500000 { cortex_m::asm::nop(); }
    }

    // Initialize and start scheduler
    // LED OFF = Starting scheduler initialization
    gpio_write(false);
    for _ in 0..500000 { cortex_m::asm::nop(); }
    

    let scheduler = scheduler();
    scheduler.initialize();

    // LED ON = Scheduler initialized, ready to start
    gpio_write(true);
    for _ in 0..500000 { cortex_m::asm::nop(); }
    
    // LED OFF = Starting scheduler
    gpio_write(false);
    for _ in 0..500000 { cortex_m::asm::nop(); }
    

    scheduler.start();
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