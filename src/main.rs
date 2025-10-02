//! STM32F446 FreeRTOS-Style Mini-OS
//!
//! A FreeRTOS-style RTOS implementation following FreeRTOS-Kernel patterns
//! with proper context switching, task management, and hardware abstraction.

#![no_std]
#![no_main]
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};
use core::arch::global_asm;
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::exception;
use core::ffi::c_void;

// ===== FreeRTOS-Style Configuration =====

/// System clock frequency (16 MHz HSI)
pub const SYSTEM_CLOCK_HZ: u32 = 16_000_000;

// FreeRTOS Configuration Constants
pub const configUSE_PREEMPTION: u32 = 1;
pub const configUSE_TASK_NOTIFICATIONS: u32 = 1;
pub const configUSE_16_BIT_TICKS: u32 = 0;
pub const configMAX_PRIORITIES: usize = 5;
pub const configMINIMAL_STACK_SIZE: usize = 64; // Stack size in words
pub const configMAX_TASK_NAME_LEN: usize = 16;
pub const configUSE_TRACE_FACILITY: u32 = 1;
pub const configCHECK_FOR_STACK_OVERFLOW: u32 = 2;

/// Maximum number of tasks (application specific)
pub const MAX_TASKS: usize = 3;

/// Interrupt priorities (following ARM Cortex-M4 conventions)
pub const configKERNEL_INTERRUPT_PRIORITY: u8 = 255;        // Lowest priority
pub const configMAX_SYSCALL_INTERRUPT_PRIORITY: u8 = 191;   // Higher than kernel

// Additional Constants for Embedded System
pub const configCPU_CLOCK_HZ: u32 = 84_000_000;
pub const configTICK_RATE_HZ: u32 = 1000;

// Stack size constants
pub const TASK_STACK_SIZE_WORDS: usize = configMINIMAL_STACK_SIZE;
pub const KERNEL_STACK_SIZE_WORDS: usize = 64;
pub const TASK_STACK_SIZE_BYTES: usize = TASK_STACK_SIZE_WORDS * 4;

// Context size constants
pub const SW_CONTEXT_SIZE: usize = 8;  // Software context (R4-R11)
pub const HW_CONTEXT_SIZE: usize = 8;  // Hardware context (R0-R3, R12, LR, PC, xPSR)
pub const STACK_GUARD_SIZE: usize = 4;  // Guard words
pub const STACK_GUARD_PATTERN: u32 = 0xDEADBEEF;

// Memory layout constants
pub const SRAM_START: u32 = 0x20000000;
pub const SRAM_SIZE: u32 = 128 * 1024; // 128KB
pub const SRAM_END: u32 = SRAM_START + SRAM_SIZE;
pub const APP_STACK_START: u32 = SRAM_START + 0x8000; // Start app stacks at 32KB offset

// System control constants
pub const SCB_SHPR3: u32 = 0xE000ED20;
pub const PENDSV_PRIORITY: u32 = 0xFF;
pub const SYSTICK_PRIORITY: u32 = 0xFF;
pub const SYSTICK_RELOAD_10MS: u32 = (configCPU_CLOCK_HZ / 100) - 1; // 10ms ticks


/// Stack canary for overflow detection
pub const STACK_CANARY_VALUE: u32 = 0xDEADBEEF;

/// FreeRTOS return codes
pub const pdTRUE: BaseType_t = 1;
pub const pdFALSE: BaseType_t = 0;
pub const pdPASS: BaseType_t = 1;
pub const pdFAIL: BaseType_t = 0;

/// Special delay value
pub const portMAX_DELAY: TickType_t = 0xFFFFFFFF;

// ===== FreeRTOS-Style Type Definitions =====

/// Stack type for ARM Cortex-M4 (32-bit)
pub type StackType_t = u32;

/// Base type for ARM (32-bit long)
pub type BaseType_t = i32;

/// Unsigned base type
pub type UBaseType_t = u32;

/// Tick type (32-bit for our config)
pub type TickType_t = u32;

/// Task function pointer type
pub type TaskFunction_t = unsafe extern "C" fn(*mut c_void) -> !;

/// Task handle (pointer to TCB)
pub type TaskHandle_t = *mut c_void;

// ===== FreeRTOS-Style Enumerations =====

/// Task states (following FreeRTOS eTaskState)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum eTaskState {
    eRunning = 0,      // A task is querying the state of itself, so must be running
    eReady,            // The task being queried is in a ready list
    eBlocked,          // The task being queried is in the Blocked state
    eSuspended,        // The task being queried is in the Suspended state
    eDeleted,          // The task being queried has been deleted
    eInvalid,          // Used as an 'invalid state' value
}

/// Task notification states
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum eNotifyState {
    eNotWaitingNotification = 0,
    eWaitingNotification,
}

// ===== FreeRTOS-Style List Structures =====

/// Mini list item (for list end marker)
#[derive(Copy, Clone)]
#[repr(C)]
pub struct MiniListItem_t {
    pub item_value: TickType_t,
    pub p_next: *mut ListItem_t,
    pub p_previous: *mut ListItem_t,
}

impl MiniListItem_t {
    pub const fn new() -> Self {
        Self {
            item_value: 0,
            p_next: core::ptr::null_mut(),
            p_previous: core::ptr::null_mut(),
        }
    }
}

/// List item structure (following FreeRTOS ListItem_t)
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ListItem_t {
    pub item_value: TickType_t,
    pub p_next: *mut ListItem_t,
    pub p_previous: *mut ListItem_t,
    pub p_owner: *mut c_void,
    pub p_container: *mut List_t,
}

impl ListItem_t {
    pub const fn new() -> Self {
        Self {
            item_value: 0,
            p_next: core::ptr::null_mut(),
            p_previous: core::ptr::null_mut(),
            p_owner: core::ptr::null_mut(),
            p_container: core::ptr::null_mut(),
        }
    }
}

/// List structure (following FreeRTOS List_t)
#[derive(Copy, Clone)]
#[repr(C)]
pub struct List_t {
    pub number_of_items: UBaseType_t,
    pub p_index: *mut ListItem_t,
    pub end: MiniListItem_t,
}

impl List_t {
    pub const fn new() -> Self {
        Self {
            number_of_items: 0,
            p_index: core::ptr::null_mut(),
            end: MiniListItem_t::new(),
        }
    }
}

// ===== FreeRTOS-Style Task Control Block =====

/// Task Control Block (following FreeRTOS tskTCB structure)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct tskTCB {
    /// Task's stack pointer - MUST BE FIRST FIELD (ARM requirement)
    pub p_top_of_stack: *mut StackType_t,

    /// Generic list item for ready/blocked lists
    pub generic_list_item: ListItem_t,

    /// Event list item for event-based blocking
    pub event_list_item: ListItem_t,

    /// Task priority (0 = lowest)
    pub priority: UBaseType_t,

    /// Pointer to start of stack
    pub p_stack: *mut StackType_t,

    /// Task name for debugging
    pub task_name: [u8; configMAX_TASK_NAME_LEN],

    /// Stack size in words
    pub stack_depth: u16,

    /// Task number for trace facility
    pub task_number: UBaseType_t,

    /// Base priority (for priority inheritance)
    pub base_priority: UBaseType_t,

    /// Mutex held count (for priority inheritance)
    pub mutexes_held: UBaseType_t,

    /// Task notification value
    pub notification_value: u32,

    /// Task notification state
    pub notification_state: eNotifyState,
}

impl tskTCB {
    pub const fn new() -> Self {
        Self {
            p_top_of_stack: core::ptr::null_mut(),
            generic_list_item: ListItem_t::new(),
            event_list_item: ListItem_t::new(),
            priority: 0,
            p_stack: core::ptr::null_mut(),
            task_name: [0; configMAX_TASK_NAME_LEN],
            stack_depth: 0,
            task_number: 0,
            base_priority: 0,
            mutexes_held: 0,
            notification_value: 0,
            notification_state: eNotifyState::eNotWaitingNotification,
        }
    }
}

// ===== FreeRTOS-Style List Management Functions =====

/// Initialize a list (following FreeRTOS vListInitialise)
unsafe fn v_list_initialise(p_list: *mut List_t) {
    unsafe {
        // The list end item value is the maximum possible value for a TickType_t
        (*p_list).end.item_value = portMAX_DELAY;

        // The list end item's previous and next pointers point to itself
        (*p_list).end.p_next = &raw mut (*p_list).end as *mut MiniListItem_t as *mut ListItem_t;
        (*p_list).end.p_previous = &raw mut (*p_list).end as *mut MiniListItem_t as *mut ListItem_t;

        // Initialize the list end item as the index
        (*p_list).p_index = &raw mut (*p_list).end as *mut MiniListItem_t as *mut ListItem_t;

        // No items in the list yet
        (*p_list).number_of_items = 0;
    }
}

/// Insert a list item at the end of a list (following FreeRTOS vListInsertEnd)
unsafe fn v_list_insert_end(p_list: *mut List_t, p_new_list_item: *mut ListItem_t) {
    unsafe {
        let p_index = (*p_list).p_index;

        // The new list item goes between the index and the item before the index
        (*p_new_list_item).p_next = p_index;
        (*p_new_list_item).p_previous = (*p_index).p_previous;

        // Update the links
        (*(*p_index).p_previous).p_next = p_new_list_item;
        (*p_index).p_previous = p_new_list_item;

        // The new item belongs to this list
        (*p_new_list_item).p_container = p_list;

        // Increment the number of items
        (*p_list).number_of_items += 1;
    }
}

/// Insert a list item in the correct position based on its value (following FreeRTOS vListInsert)
unsafe fn v_list_insert(p_list: *mut List_t, p_new_list_item: *mut ListItem_t) {
    unsafe {
        let value_of_insertion = (*p_new_list_item).item_value;

        // Special case: if the value is portMAX_DELAY, insert at the end
        if value_of_insertion == portMAX_DELAY {
            let p_iterator = (*p_list).end.p_previous as *mut ListItem_t;
            (*p_new_list_item).p_next = &raw mut (*p_list).end as *mut MiniListItem_t as *mut ListItem_t;
            (*p_new_list_item).p_previous = p_iterator;
            (*p_iterator).p_next = p_new_list_item;
            (*p_list).end.p_previous = p_new_list_item as *mut ListItem_t;
        } else {
            // Find the correct position in the list
            let mut p_iterator = &raw mut (*p_list).end as *mut MiniListItem_t as *mut ListItem_t;

            loop {
                p_iterator = (*p_iterator).p_next;
                if (*p_iterator).item_value >= value_of_insertion {
                    break;
                }
            }

            // Insert the new item before the iterator
            (*p_new_list_item).p_next = p_iterator;
            (*p_new_list_item).p_previous = (*p_iterator).p_previous;
            (*(*p_iterator).p_previous).p_next = p_new_list_item;
            (*p_iterator).p_previous = p_new_list_item;
        }

        // The new item belongs to this list
        (*p_new_list_item).p_container = p_list;

        // Increment the number of items
        (*p_list).number_of_items += 1;
    }
}

/// Remove a list item from its container list (following FreeRTOS uxListRemove)
unsafe fn ux_list_remove(p_item_to_remove: *mut ListItem_t) -> UBaseType_t {
    unsafe {
        let p_list = (*p_item_to_remove).p_container;

        // Remove the item from the list
        (*(*p_item_to_remove).p_next).p_previous = (*p_item_to_remove).p_previous;
        (*(*p_item_to_remove).p_previous).p_next = (*p_item_to_remove).p_next;

        // If the list index was pointing to this item, move it to the previous item
        if (*p_list).p_index == p_item_to_remove {
            (*p_list).p_index = (*p_item_to_remove).p_previous;
        }

        // The item is no longer in any list
        (*p_item_to_remove).p_container = core::ptr::null_mut();

        // Decrement the number of items and return the new count
        (*p_list).number_of_items -= 1;
        (*p_list).number_of_items
    }
}

/// Check if a list is empty (following FreeRTOS listIS_EMPTY)
#[inline(always)]
unsafe fn list_is_empty(p_list: *const List_t) -> bool {
    unsafe {
        (*p_list).number_of_items == 0
    }
}

/// Get the head entry of a list (following FreeRTOS listGET_HEAD_ENTRY)
#[inline(always)]
unsafe fn list_get_head_entry(p_list: *const List_t) -> *mut ListItem_t {
    unsafe {
        (*p_list).end.p_next
    }
}

/// Get the owner of a list item (following FreeRTOS listGET_LIST_ITEM_OWNER)
#[inline(always)]
unsafe fn list_get_list_item_owner(p_list_item: *const ListItem_t) -> *mut c_void {
    unsafe {
        (*p_list_item).p_owner
    }
}

/// Set the owner of a list item (following FreeRTOS listSET_LIST_ITEM_OWNER)
#[inline(always)]
unsafe fn list_set_list_item_owner(p_list_item: *mut ListItem_t, p_owner: *mut c_void) {
    unsafe {
        (*p_list_item).p_owner = p_owner;
    }
}

/// Get the value of a list item (following FreeRTOS listGET_LIST_ITEM_VALUE)
#[inline(always)]
unsafe fn list_get_list_item_value(p_list_item: *const ListItem_t) -> TickType_t {
    unsafe {
        (*p_list_item).item_value
    }
}

/// Set the value of a list item (following FreeRTOS listSET_LIST_ITEM_VALUE)
#[inline(always)]
unsafe fn list_set_list_item_value(p_list_item: *mut ListItem_t, item_value: TickType_t) {
    unsafe {
        (*p_list_item).item_value = item_value;
    }
}

// ===== Hardware Constants =====

/// ARM Cortex-M4 initial stack pointer and exception return values
pub const PORT_INITIAL_XPSR: StackType_t = 0x01000000; // Thumb state bit set
pub const PORT_START_ADDRESS_MASK: StackType_t = 0xFFFFFFFE;

/// SysTick configuration
pub const TIME_SLICE_MS: u32 = 1000 / configTICK_RATE_HZ;

// ===== FreeRTOS-Style Static Memory Allocation =====

/// Global scheduler state variables
static mut UX_SCHEDULER_RUNNING: BaseType_t = pdFALSE;
static mut UX_CURRENT_NUMBER_OF_TASKS: UBaseType_t = 0;
static mut UX_TICK_COUNT: TickType_t = 0;
static mut UX_YIELD_PENDING: BaseType_t = pdFALSE;
static mut UX_NUM_OF_OVERFLOWS: BaseType_t = 0;
static mut UX_NEXT_TASK_UNBLOCK_TIME: TickType_t = 0;

/// Current task pointer (must match assembly expectations)
static mut PX_CURRENT_TCB: *mut tskTCB = core::ptr::null_mut();

/// Ready task lists - one for each priority level (following FreeRTOS pattern)
static mut PX_READY_TASK_LISTS: [List_t; configMAX_PRIORITIES] = [List_t::new(); configMAX_PRIORITIES];

/// Delayed task lists (for vTaskDelay)
static mut X_DELAYED_TASK_LIST1: List_t = List_t::new();
static mut X_DELAYED_TASK_LIST2: List_t = List_t::new();
static mut PX_DELAYED_TASK_LIST: *mut List_t = core::ptr::null_mut();
static mut PX_OVERFLOW_DELAYED_TASK_LIST: *mut List_t = core::ptr::null_mut();

/// Suspended task list
static mut X_SUSPENDED_TASK_LIST: List_t = List_t::new();

/// Task stacks - statically allocated (following static allocation pattern)
static mut TASK_STACKS: [[StackType_t; configMINIMAL_STACK_SIZE]; MAX_TASKS] =
    [[0; configMINIMAL_STACK_SIZE]; MAX_TASKS];

/// Task Control Blocks - statically allocated
static mut TASK_TCBS: [tskTCB; MAX_TASKS] = [tskTCB::new(); MAX_TASKS];

/// Idle task stack and TCB
static mut IDLE_TASK_STACK: [StackType_t; configMINIMAL_STACK_SIZE] = [0; configMINIMAL_STACK_SIZE];
static mut IDLE_TASK_TCB: tskTCB = tskTCB::new();

/// Critical nesting counter for interrupt management
static mut UX_CRITICAL_NESTING: UBaseType_t = 0;

/// Task counter for unique task numbers
static mut UX_TASK_NUMBER: UBaseType_t = 0;

// ===== EXC_RETURN Values =====

/// EXC_RETURN for Thread mode, PSP, no FPU context
pub const EXC_RETURN_THREAD_PSP: u32 = 0xFFFFFFFD;

/// EXC_RETURN for Thread mode, MSP
pub const EXC_RETURN_THREAD_MSP: u32 = 0xFFFFFFF9;

// ===== ARM Cortex-M Hardware Registers (Following FreeRTOS Port) =====

/// System Control Block (SCB) registers
pub const PORT_SCB_BASE: u32 = 0xE000ED00;
pub const PORT_SCB_ICSR: *mut u32 = (PORT_SCB_BASE + 0x04) as *mut u32;
pub const PORT_SCB_VTOR: *mut u32 = (PORT_SCB_BASE + 0x08) as *mut u32;
pub const PORT_SCB_AIRCR: *mut u32 = (PORT_SCB_BASE + 0x0C) as *mut u32;
pub const PORT_SCB_SHPR3: *mut u32 = (PORT_SCB_BASE + 0x20) as *mut u32;

/// NVIC interrupt control
pub const PORT_NVIC_PENDSVSET_BIT: u32 = 0x10000000;
pub const PORT_NVIC_PENDSVCLR_BIT: u32 = 0x08000000;
pub const PORT_NVIC_PEND_SYSTICK_SET_BIT: u32 = 0x04000000;
pub const PORT_NVIC_PEND_SYSTICK_CLEAR_BIT: u32 = 0x02000000;

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
    let stack_start = APP_STACK_START + (task_id as u32 * TASK_STACK_SIZE_BYTES as u32);
    let stack_end = stack_start + TASK_STACK_SIZE_BYTES as u32;
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

// ===== FreeRTOS-Style PendSV Handler (ARM Cortex-M4) =====

global_asm!(
    r#"
    .syntax unified
    .thumb
    .text

    .global PendSV_Handler
    .type   PendSV_Handler, %function
    .thumb_func

PendSV_Handler:
    /* Disable interrupts during context switch */
    mrs     r0, psp
    isb

    /* Get current task's TCB address */
    ldr     r3, =pxCurrentTCB
    ldr     r2, [r3]

    /* Check if this is the first task (pxCurrentTCB == NULL) */
    cmp     r2, #0
    beq     restore_first_task

    /* Save remaining core registers (r4-r11) on process stack */
    stmdb   r0!, {{r4-r11}}

    /* Save the new top of stack into the TCB */
    str     r0, [r2]

save_context_complete:
    /* Call vTaskSwitchContext to select next task */
    stmdb   sp!, {{r3, r14}}
    mov     r0, #191
    msr     basepri, r0
    dsb
    isb
    bl      vTaskSwitchContext
    mov     r0, #0
    msr     basepri, r0
    ldmia   sp!, {{r3, r14}}

    /* Load the new current TCB */
    ldr     r1, [r3]
    ldr     r0, [r1]

restore_first_task:
    /* Restore core registers (r4-r11) */
    ldmia   r0!, {{r4-r11}}

    /* Update PSP */
    msr     psp, r0
    isb

    /* Return to task */
    bx      r14
"#,
);

// ===== FreeRTOS-Style Critical Section Management =====

/// Enter critical section (following FreeRTOS portENTER_CRITICAL)
#[inline(always)]
unsafe fn port_enter_critical() {
    unsafe {
        port_disable_interrupts();
        UX_CRITICAL_NESTING += 1;
        core::arch::asm!("dsb", "isb", options(nomem, nostack));
    }
}

/// Exit critical section (following FreeRTOS portEXIT_CRITICAL)
#[inline(always)]
unsafe fn port_exit_critical() {
    unsafe {
        UX_CRITICAL_NESTING = UX_CRITICAL_NESTING.saturating_sub(1);
        if UX_CRITICAL_NESTING == 0 {
            port_enable_interrupts();
        }
    }
}

/// Disable interrupts (following FreeRTOS portmacro.h)
#[inline(always)]
unsafe fn port_disable_interrupts() {
    unsafe {
        core::arch::asm!(
            "mov r0, #191",
            "msr basepri, r0",
            "dsb",
            "isb",
            out("r0") _,
            options(nomem, nostack)
        );
    }
}

/// Enable interrupts (following FreeRTOS portmacro.h)
#[inline(always)]
unsafe fn port_enable_interrupts() {
    unsafe {
        core::arch::asm!(
            "mov r0, #0",
            "msr basepri, r0",
            out("r0") _,
            options(nomem, nostack)
        );
    }
}

/// Yield the processor (following FreeRTOS portYIELD)
#[inline(always)]
unsafe fn port_yield() {
    unsafe {
        // Set PendSV interrupt pending
        core::ptr::write_volatile(PORT_SCB_ICSR, PORT_NVIC_PENDSVSET_BIT);
        // Memory barrier to ensure write completes
        core::arch::asm!("dsb", "isb", options(nomem, nostack));
    }
}

// ===== FreeRTOS-Style Task Switching Functions =====

/// Context switching function called by PendSV handler (following FreeRTOS vTaskSwitchContext)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vTaskSwitchContext() {
    unsafe {
        if UX_SCHEDULER_RUNNING != pdFALSE {
            // Check if a yield is pending
            UX_YIELD_PENDING = pdFALSE;

            // Find the highest priority ready task
            prv_get_highest_priority_ready_task();
        }
    }
}

/// Get the highest priority ready task (following FreeRTOS task selection logic)
unsafe fn prv_get_highest_priority_ready_task() {
    unsafe {
        // Find the highest priority with a ready task
        for priority in (0..configMAX_PRIORITIES).rev() {
            if !list_is_empty(&PX_READY_TASK_LISTS[priority]) {
                let list_item = list_get_head_entry(&PX_READY_TASK_LISTS[priority]);
                let new_tcb = list_get_list_item_owner(list_item) as *mut tskTCB;

                // Switch to the new task if different from current
                if new_tcb != PX_CURRENT_TCB {
                    PX_CURRENT_TCB = new_tcb;
                }
                return;
            }
        }

        // If no ready tasks found, use idle task
        PX_CURRENT_TCB = &raw mut IDLE_TASK_TCB;
    }
}

/// Stack initialization function (following FreeRTOS pxPortInitialiseStack)
unsafe fn px_port_initialise_stack(
    p_top_of_stack: *mut StackType_t,
    p_code: TaskFunction_t,
    p_parameters: *mut c_void,
) -> *mut StackType_t {
    unsafe {
        let mut p_top_of_stack = p_top_of_stack;

        // Simulate the stack frame as it would be created by a context switch interrupt
        // The order matches what the hardware pushes onto the stack automatically

        // Simulate hardware stack frame (automatically saved by Cortex-M)
        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = PORT_INITIAL_XPSR; // xPSR

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = (p_code as usize as StackType_t) & PORT_START_ADDRESS_MASK; // PC

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // LR (R14)

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R12

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R3

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R2

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R1

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = p_parameters as usize as StackType_t; // R0 (first parameter)

        // Simulate software stack frame (manually saved by PendSV)
        // This matches the stmdb instruction in PendSV_Handler
        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R11

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R10

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R9

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R8

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R7

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R6

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R5

        p_top_of_stack = p_top_of_stack.offset(-1);
        *p_top_of_stack = 0; // R4

        p_top_of_stack
    }
}

// ===== FreeRTOS-Style Task Management =====

/// Create a new task (following FreeRTOS xTaskCreate)
pub unsafe fn x_task_create(
    p_task_code: TaskFunction_t,
    task_name: &str,
    stack_depth: u16,
    p_parameters: *mut c_void,
    priority: UBaseType_t,
    p_created_task: *mut TaskHandle_t,
) -> BaseType_t {
    unsafe {
        let mut return_value = pdFAIL;

        // Ensure we don't exceed maximum tasks
        if UX_CURRENT_NUMBER_OF_TASKS < MAX_TASKS as UBaseType_t {
            let task_index = UX_CURRENT_NUMBER_OF_TASKS as usize;
            let p_new_tcb = &raw mut TASK_TCBS[task_index];

            // Initialize the stack
            let p_stack = core::ptr::addr_of_mut!(TASK_STACKS[task_index]) as *mut StackType_t;
            let p_stack_end = p_stack.add(configMINIMAL_STACK_SIZE - 1);

            // Initialize the TCB
            prv_initialise_tcb(
                p_new_tcb,
                task_name,
                priority,
                p_stack,
                stack_depth,
            );

            // Initialize the stack for the task
            (*p_new_tcb).p_top_of_stack = px_port_initialise_stack(
                p_stack_end,
                p_task_code,
                p_parameters,
            );

            if !p_created_task.is_null() {
                *p_created_task = p_new_tcb as *mut tskTCB as *mut c_void;
            }

            // Add task to the ready list
            prv_add_new_task_to_ready_list(p_new_tcb);

            return_value = pdPASS;
            UX_CURRENT_NUMBER_OF_TASKS += 1;

            rprintln!("[TASK] Created task '{}' priority {} stack {:?}",
                      task_name, priority, (*p_new_tcb).p_top_of_stack);
        } else {
            rprintln!("[ERROR] Maximum number of tasks reached");
        }

        return_value
    }
}

/// Initialize TCB with basic values (following FreeRTOS prvInitialiseTCBVariables)
unsafe fn prv_initialise_tcb(
    p_tcb: *mut tskTCB,
    task_name: &str,
    priority: UBaseType_t,
    p_stack: *mut StackType_t,
    stack_depth: u16,
) {
    unsafe {
        // Initialize the list items
        list_set_list_item_owner(&raw mut (*p_tcb).generic_list_item, p_tcb as *mut c_void);
        list_set_list_item_value(&raw mut (*p_tcb).generic_list_item, configMAX_PRIORITIES as TickType_t - priority as TickType_t);

        list_set_list_item_owner(&raw mut (*p_tcb).event_list_item, p_tcb as *mut c_void);
        list_set_list_item_value(&raw mut (*p_tcb).event_list_item, configMAX_PRIORITIES as TickType_t - priority as TickType_t);

        // Set priority
        (*p_tcb).priority = priority;
        (*p_tcb).base_priority = priority;

        // Copy task name
        let name_bytes = task_name.as_bytes();
        let copy_length = core::cmp::min(name_bytes.len(), configMAX_TASK_NAME_LEN - 1);
        (&mut (*p_tcb).task_name)[..copy_length].copy_from_slice(&name_bytes[..copy_length]);
        (*p_tcb).task_name[copy_length] = 0; // Null terminator

        // Stack information
        (*p_tcb).p_stack = p_stack;
        (*p_tcb).stack_depth = stack_depth;

        // Task number for tracing
        (*p_tcb).task_number = UX_TASK_NUMBER;
        UX_TASK_NUMBER += 1;

        // Initialize notification state
        (*p_tcb).notification_state = eNotifyState::eNotWaitingNotification;
        (*p_tcb).notification_value = 0;
    }
}

/// Add a new task to the ready list (following FreeRTOS prvAddNewTaskToReadyList)
unsafe fn prv_add_new_task_to_ready_list(p_new_tcb: *mut tskTCB) {
    unsafe {
        port_enter_critical();
        {
            let priority = (*p_new_tcb).priority;

            // Add the task to the ready list for its priority
            v_list_insert_end(&raw mut PX_READY_TASK_LISTS[priority as usize], &raw mut (*p_new_tcb).generic_list_item);

            rprintln!("[SCHEDULER] Added task to ready list priority {}", priority);

            // If this is the first task or the scheduler is not running yet,
            // and this task has higher priority than current, make it current
            if UX_CURRENT_NUMBER_OF_TASKS == 1 || PX_CURRENT_TCB.is_null() {
                PX_CURRENT_TCB = p_new_tcb;
                rprintln!("[SCHEDULER] Set as current task");
            }
        }
        port_exit_critical();
    }
}

/// FreeRTOS-style tick increment function
pub unsafe fn x_task_increment_tick() -> BaseType_t {
    // For now, just return pdTRUE to always force context switch on each tick
    // In full FreeRTOS, this would handle delays, timeouts, etc.
    pdTRUE
}

pub unsafe fn v_task_start_scheduler() {
    unsafe {
        // Create the idle task
        prv_create_idle_task();

        // Initialize the scheduler lists
        prv_initialise_task_lists();

        // Set up the hardware for context switching
        if px_port_start_scheduler() == pdTRUE {
            // The scheduler has started successfully
            UX_SCHEDULER_RUNNING = pdTRUE;
        } else {
            // Failed to start scheduler
            rprintln!("[ERROR] Failed to start scheduler");
        }
    }
}

/// Initialize the task lists (following FreeRTOS prvInitialiseTaskLists)
unsafe fn prv_initialise_task_lists() {
    unsafe {
        // Initialize ready lists for each priority
        for i in 0..configMAX_PRIORITIES {
            v_list_initialise(&raw mut PX_READY_TASK_LISTS[i]);
        }

        // Initialize delayed task lists
        v_list_initialise(&raw mut X_DELAYED_TASK_LIST1);
        v_list_initialise(&raw mut X_DELAYED_TASK_LIST2);
        v_list_initialise(&raw mut X_SUSPENDED_TASK_LIST);

        // Set up delayed list pointers
        PX_DELAYED_TASK_LIST = &raw mut X_DELAYED_TASK_LIST1;
        PX_OVERFLOW_DELAYED_TASK_LIST = &raw mut X_DELAYED_TASK_LIST2;

        rprintln!("[SCHEDULER] Task lists initialized");
    }
}

/// Create the idle task (following FreeRTOS prvCreateIdleTask)
unsafe fn prv_create_idle_task() {
    unsafe {
        // Initialize idle task name
        let idle_name = "IDLE\0";
        let name_bytes = idle_name.as_bytes();
        IDLE_TASK_TCB.task_name[..name_bytes.len()].copy_from_slice(name_bytes);

        // Set up idle task
        IDLE_TASK_TCB.priority = 0; // Lowest priority
        IDLE_TASK_TCB.base_priority = 0;
        IDLE_TASK_TCB.p_stack = core::ptr::addr_of_mut!(IDLE_TASK_STACK) as *mut StackType_t;
        IDLE_TASK_TCB.stack_depth = configMINIMAL_STACK_SIZE as u16;

        // Initialize idle task stack
        let p_stack_end = (core::ptr::addr_of_mut!(IDLE_TASK_STACK) as *mut StackType_t).add(configMINIMAL_STACK_SIZE - 1);
        IDLE_TASK_TCB.p_top_of_stack = px_port_initialise_stack(
            p_stack_end,
            prv_idle_task,
            core::ptr::null_mut(),
        );

        // Set up list items
        list_set_list_item_owner(&raw mut IDLE_TASK_TCB.generic_list_item, &raw mut IDLE_TASK_TCB as *mut tskTCB as *mut c_void);
        list_set_list_item_value(&raw mut IDLE_TASK_TCB.generic_list_item, 0); // Lowest priority

        list_set_list_item_owner(&raw mut IDLE_TASK_TCB.event_list_item, &raw mut IDLE_TASK_TCB as *mut tskTCB as *mut c_void);

        // Add idle task to ready list
        v_list_insert_end(&raw mut PX_READY_TASK_LISTS[0], &raw mut IDLE_TASK_TCB.generic_list_item);

        rprintln!("[IDLE] Idle task created");
    }
}

/// Idle task function (following FreeRTOS idle task pattern)
unsafe extern "C" fn prv_idle_task(_p_parameters: *mut c_void) -> ! {
    unsafe {
        loop {
            // Idle task just does nothing and yields
            // In a full implementation, this could do housekeeping tasks
            core::arch::asm!("nop");
            core::arch::asm!("wfi"); // Wait for interrupt to save power
        }
    }
}

/// Start the scheduler (hardware-specific port initialization)
unsafe fn px_port_start_scheduler() -> BaseType_t {
    unsafe {
        // Set interrupt priorities
        prv_setup_timer_interrupt();

        // Set PendSV and SysTick to the lowest interrupt priority
        core::ptr::write_volatile(PORT_SCB_SHPR3,
            (configKERNEL_INTERRUPT_PRIORITY as u32) << 16 | // PendSV
            (configKERNEL_INTERRUPT_PRIORITY as u32) << 24   // SysTick
        );

        // Start first task by triggering PendSV
        if !PX_CURRENT_TCB.is_null() {
            port_yield();
            pdTRUE
        } else {
            pdFAIL
        }
    }
}

/// Setup timer interrupt (SysTick)
unsafe fn prv_setup_timer_interrupt() {
    unsafe {
        // Configure SysTick for the tick interrupt
        let reload_value = configCPU_CLOCK_HZ / configTICK_RATE_HZ;

        // Set reload value
        core::ptr::write_volatile((0xE000E014) as *mut u32, reload_value - 1);

        // Clear current value
        core::ptr::write_volatile((0xE000E018) as *mut u32, 0);

        // Enable SysTick, use processor clock, enable interrupt
        core::ptr::write_volatile((0xE000E010) as *mut u32, 0x07);

        rprintln!("[SYSTICK] Configured for {} Hz tick rate", configTICK_RATE_HZ);
    }
}

// ===== Tasks =====

/// Task 0: LED control task (FreeRTOS-style)
#[unsafe(no_mangle)]
pub extern "C" fn task0_entry(_parameters: *mut c_void) -> ! {
    rprintln!("[TASK0] Starting FreeRTOS LED control task - Priority 2");

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

/// Task 1: General purpose task (FreeRTOS-style)
#[unsafe(no_mangle)]
pub extern "C" fn task1_entry(_parameters: *mut c_void) -> ! {
    rprintln!("[TASK1] Starting FreeRTOS general task - Priority 1");

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

/// Task 2: Background task (FreeRTOS-style)
#[unsafe(no_mangle)]
pub extern "C" fn task2_entry(_parameters: *mut c_void) -> ! {
    rprintln!("[TASK2] Starting FreeRTOS background task - Priority 0");

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

/// SysTick handler - performs FreeRTOS-style preemptive scheduling
#[unsafe(no_mangle)]
pub extern "C" fn SysTick() {
    // Increment the tick count
    unsafe {
        UX_TICK_COUNT = UX_TICK_COUNT.wrapping_add(1);
    }

    // Check if we need to context switch to higher priority task
    // This will be handled by vTaskSwitchContext when PendSV is triggered
    if unsafe { x_task_increment_tick() } != pdFALSE {
        // A context switch is needed - trigger PendSV
        trigger_pendsv();
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

    rprintln!("[FREERTOS] Starting FreeRTOS-style embedded OS...");

    // Initialize MSP/PSP stack separation
    unsafe {
        initialize_msp_psp_separation();
    }
    rprintln!("[MAIN] MSP/PSP initialization completed");

    // Initialize GPIO system
    unsafe {
        init_gpio();
        rprintln!("[MAIN] GPIO initialization completed");
        gpio_write(true); // LED ON to show GPIO ready
    }

    // Initialize the FreeRTOS-style scheduler
    rprintln!("[SCHEDULER] Initializing task lists...");
    unsafe {
        prv_initialise_task_lists();
    }
    rprintln!("[SCHEDULER] Task lists initialized");

    // Create tasks using FreeRTOS-style API
    rprintln!("[TASKS] Creating application tasks...");

    // Create Task 0 - LED control task (High Priority)
    let mut task0_handle: TaskHandle_t = core::ptr::null_mut();
    let result0 = unsafe {
        x_task_create(
            task0_entry,
            "Task0",
            configMINIMAL_STACK_SIZE as u16,
            core::ptr::null_mut(),
            2, // High priority
            &mut task0_handle,
        )
    };
    if result0 != pdPASS {
        panic!("[ERROR] Failed to create Task 0");
    }
    rprintln!("[TASKS] Task 0 created successfully");

    // Create Task 1 - General task (Medium Priority)
    let mut task1_handle: TaskHandle_t = core::ptr::null_mut();
    let result1 = unsafe {
        x_task_create(
            task1_entry,
            "Task1",
            configMINIMAL_STACK_SIZE as u16,
            core::ptr::null_mut(),
            1, // Medium priority
            &mut task1_handle,
        )
    };
    if result1 != pdPASS {
        panic!("[ERROR] Failed to create Task 1");
    }
    rprintln!("[TASKS] Task 1 created successfully");

    // Create Task 2 - Background task (Low Priority)
    let mut task2_handle: TaskHandle_t = core::ptr::null_mut();
    let result2 = unsafe {
        x_task_create(
            task2_entry,
            "Task2",
            configMINIMAL_STACK_SIZE as u16,
            core::ptr::null_mut(),
            0, // Low priority
            &mut task2_handle,
        )
    };
    if result2 != pdPASS {
        panic!("[ERROR] Failed to create Task 2");
    }
    rprintln!("[TASKS] Task 2 created successfully");

    rprintln!("[SCHEDULER] All application tasks created");

    rprintln!("[SCHEDULER] Starting FreeRTOS scheduler...");

    // Start the scheduler - this should never return
    unsafe {
        v_task_start_scheduler();
    }

    // Should never reach here if scheduler starts successfully
    panic!("[ERROR] Scheduler failed to start!");
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