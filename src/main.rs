// mini_os_app_framework.rs
#![no_std]
#![no_main]
#![allow(dead_code)]

use cortex_m_rt::entry;
use panic_halt as _;
use rtt_target::{rprintln, rtt_init_print};
use stm32f4 as _; // Required for memory layout and vector table

const CYCLES_PER_MS_ESTIMATE: u32 = 16_000;

#[derive(Copy, Clone, Debug)]
pub enum GpioPin {
    Led1,
}

pub trait Syscalls {
    fn gpio_write(&mut self, pin: GpioPin, high: bool);
    fn gpio_toggle(&mut self, pin: GpioPin);
    fn sleep_ms(&mut self, ms: u32);
    fn now_ms(&self) -> u64;
}

#[inline(always)]
fn gpio_pin_to_idx(pin: GpioPin) -> u32 {
    match pin {
        GpioPin::Led1 => 0,
    }
}

// ───────────── BOARD LAYER ─────────────

mod board {
    use core::ptr::{read_volatile, write_volatile};
    use crate::{GpioPin, Syscalls};
    use cortex_m::asm::nop;
    use rtt_target::{rprintln};

    const RCC_BASE: u32 = 0x4002_3800;
    const RCC_AHB1ENR: *mut u32 = (RCC_BASE + 0x30) as *mut u32;

    pub const GPIOA_BASE: u32 = 0x4002_0000;
    pub const GPIOC_BASE: u32 = 0x4002_0800;

    const MODER_OFF: u32 = 0x00;
    const OTYPER_OFF: u32 = 0x04;
    const PUPDR_OFF: u32 = 0x0C;
    const IDR_OFF: u32 = 0x10;
    const ODR_OFF: u32 = 0x14;
    const BSRR_OFF: u32 = 0x18;

    #[inline(always)]
    const fn reg32(addr: u32) -> *mut u32 {
        addr as *mut u32
    }

    unsafe fn gpio_enable_clock(port_base: u32) {
        let bit = match port_base {
            GPIOA_BASE => 0,
            GPIOC_BASE => 2,
            _ => unreachable!(),
        };
        let mut v = unsafe { read_volatile(RCC_AHB1ENR) };
        v |= 1 << bit;
        unsafe { write_volatile(RCC_AHB1ENR, v); }
        for _ in 0..128 {
            nop();
        }
    }

    unsafe fn gpio_set_output(port_base: u32, pin: u8) {
        let shift = (pin as u32) * 2;

        let moder = reg32(port_base + MODER_OFF);
        let mut v = unsafe { read_volatile(moder) };
        v &= !(0b11 << shift);
        v |= 0b01 << shift;
        unsafe { write_volatile(moder, v) };

        let otyper = reg32(port_base + OTYPER_OFF);
        let mut v = unsafe { read_volatile(otyper) };
        v &= !(1 << pin);
        unsafe { write_volatile(otyper, v) };

        let pupdr = reg32(port_base + PUPDR_OFF);
        let mut v = unsafe { read_volatile(pupdr) };
        v &= !(0b11 << shift);
        unsafe { write_volatile(pupdr, v) };
    }

    unsafe fn gpio_set_input_pullup(port_base: u32, pin: u8) {
        let shift = (pin as u32) * 2;

        let moder = reg32(port_base + MODER_OFF);
        let mut v = unsafe { read_volatile(moder) };
        v &= !(0b11 << shift);
        unsafe { write_volatile(moder, v) };

        let pupdr = reg32(port_base + PUPDR_OFF);
        let mut v = unsafe { read_volatile(pupdr) };
        v &= !(0b11 << shift);
        v |= 0b01 << shift;
        unsafe { write_volatile(pupdr, v) };
    }

    unsafe fn gpio_write(port_base: u32, pin: u8, high: bool) {
        rprintln!("[GPIO] gpio_write called: pin={}, high={}", pin, high);
        let bsrr = reg32(port_base + BSRR_OFF);
        let val = if high { 1u32 << pin } else { 1u32 << (pin + 16) };
        unsafe { write_volatile(bsrr, val) };
    }

    unsafe fn gpio_toggle(port_base: u32, pin: u8) {
        let odr = reg32(port_base + ODR_OFF);
        let cur = unsafe { read_volatile(odr) };
        unsafe { gpio_write(port_base, pin, ((cur >> pin) & 1) == 0); }
    }

    pub struct RawPin {
        pub(crate) port_base: u32,
        pub(crate) pin: u8,
    }

    impl RawPin {
        pub const fn new(port_base: u32, pin: u8) -> Self {
            Self { port_base, pin }
        }
    }

    pub struct BoardSyscalls {
        led1: RawPin,
        btn: RawPin,
        time_ms: u64,
        cycles_per_ms: u32,
    }

    impl BoardSyscalls {
        pub const fn new(led1: RawPin, btn: RawPin, cycles_per_ms: u32) -> Self {
            Self {
                led1,
                btn,
                time_ms: 0,
                cycles_per_ms,
            }
        }

        pub unsafe fn init(&mut self) {
            unsafe { gpio_enable_clock(GPIOA_BASE); }
            unsafe { gpio_enable_clock(GPIOC_BASE); }
            unsafe { gpio_set_output(self.led1.port_base, self.led1.pin); }
            unsafe { gpio_set_input_pullup(self.btn.port_base, self.btn.pin); }
        }

        fn spin_delay(&mut self, ms: u32) {
            for _ in 0..ms {
                for _ in 0..self.cycles_per_ms {
                    nop();
                }
                self.time_ms = self.time_ms.wrapping_add(1);
            }
        }
    }

    impl Syscalls for BoardSyscalls {
        fn gpio_write(&mut self, pin: GpioPin, high: bool) {
            unsafe {
                match pin {
                    GpioPin::Led1 => gpio_write(self.led1.port_base, self.led1.pin, high),
                }
            }
        }

        fn gpio_toggle(&mut self, pin: GpioPin) {
            unsafe {
                match pin {
                    GpioPin::Led1 => gpio_toggle(self.led1.port_base, self.led1.pin),
                }
            }
        }

        fn sleep_ms(&mut self, ms: u32) {
            self.spin_delay(ms);
        }

        fn now_ms(&self) -> u64 {
            self.time_ms
        }
    }

    pub struct GpioPriv {
        pub(crate) port_base: u32,
        pub(crate) pin: u8,
    }

    impl GpioPriv {
        pub const unsafe fn new_privileged_const(port_base: u32, pin: u8) -> Self {
            Self { port_base, pin }
        }

        #[inline]
        pub fn write(&self, high: bool) {
            unsafe { gpio_write(self.port_base, self.pin, high) };
        }

        #[inline]
        pub fn toggle(&self) {
            unsafe { gpio_toggle(self.port_base, self.pin) };
        }
    }

    pub const GPIOA: u32 = GPIOA_BASE;
    pub const GPIOC: u32 = GPIOC_BASE;
}

// ───────────── CAPSULES ─────────────
mod capsules {
    #![forbid(unsafe_code)]

    use crate::{board, GpioPin};

    pub struct MuxGpio {
        led1: &'static board::GpioPriv,
        led2: &'static board::GpioPriv,
    }

    impl MuxGpio {
        pub const fn new(led1: &'static board::GpioPriv, led2: &'static board::GpioPriv) -> Self {
            Self { led1, led2 }
        }

        #[inline]
        pub fn write(&self, pin: GpioPin, high: bool) {
            match pin {
                GpioPin::Led1 => self.led1.write(high),
            }
        }

        pub fn toggle(&self, pin: GpioPin) {
            match pin {
                GpioPin::Led1 => self.led1.toggle(),
            }
        }
    }
}

// ───────────── SVC ─────────────
mod svc {
    use core::arch::{asm, global_asm};
    use core::sync::atomic::{AtomicU32, Ordering};

    use rtt_target::{rprintln};

    use crate::{GpioPin, Syscalls};

    pub mod abi {
        pub const NOW_MS: u8 = 1;
        pub const GPIO_WRITE: u8 = 2;
        pub const GPIO_TOGGLE: u8 = 3;
        pub const SLEEP_MS: u8 = 4;
    }

    #[inline(always)]
    pub fn svc_call(call_id: u8, a0: u32, a1: u32, a2: u32, a3: u32) -> u32 {
        let mut r0 = call_id as u32;
        unsafe {
            asm!(
                "svc 0",
                inlateout("r0") r0,
                in("r1") a0,
                in("r2") a1,
                in("r3") a2,
                in("r12") a3,
                options(nostack)
            );
        }
        r0
    }

    static SVC_COUNTER: AtomicU32 = AtomicU32::new(0);
    static NOW_COUNT: AtomicU32 = AtomicU32::new(0);

    pub fn svc_stats() -> (u32, u32) {
        (
            SVC_COUNTER.load(Ordering::Relaxed),
            NOW_COUNT.load(Ordering::Relaxed),
        )
    }

    #[repr(C)]
    pub struct ExceptionFrame {
        pub r0: u32,
        pub r1: u32,
        pub r2: u32,
        pub r3: u32,
        pub r12: u32,
        pub lr: u32,
        pub pc: u32,
        pub xpsr: u32,
    }

    global_asm!(
        r#"
        .global SVCall
        .type   SVCall, %function
    SVCall:
        tst     lr, #4
        ite     eq
        mrseq   r0, msp
        mrsne   r0, psp
        b       {svcrust}
    "#,
        svcrust = sym svcall_rust
    );

    extern "C" fn svcall_rust(frame: &mut ExceptionFrame) {
        let call_id = (frame.r0 & 0xFF) as u8;

        rprintln!("[SVC] svcall_rust called with ID: {}", call_id);

        SVC_COUNTER.fetch_add(1, Ordering::Relaxed);
        if call_id == abi::NOW_MS {
            NOW_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        let ret = unsafe {
            kernel_dispatch(call_id, frame.r1, frame.r2, frame.r3, frame.r12)
        };
        frame.r0 = ret;
    }

    static mut BOARD_PTR: *mut crate::board::BoardSyscalls = core::ptr::null_mut();

    pub unsafe fn register_kernel_board(p: *mut crate::board::BoardSyscalls) {
        unsafe { BOARD_PTR = p; }
    }

    unsafe fn kernel_dispatch(call_id: u8, a0: u32, a1: u32, _a2: u32, _a3: u32) -> u32 {
        let board = unsafe { &mut *BOARD_PTR };
        match call_id {
            abi::NOW_MS => board.now_ms() as u32,
            abi::GPIO_WRITE => {
                board.gpio_write(GpioPin::Led1, a1 != 0);
                0
            }
            abi::GPIO_TOGGLE => {
                board.gpio_toggle(GpioPin::Led1);
                0
            }
            abi::SLEEP_MS => {
                board.sleep_ms(a0);
                0
            }
            _ => 0xFFFF_FFFF,
        }
    }

    pub struct Client {
        board: *mut crate::board::BoardSyscalls,
    }

    impl Client {
        pub unsafe fn new(board: &mut crate::board::BoardSyscalls) -> Self {
            Self { board: board as *mut _ }
        }
    }

    impl Syscalls for Client {
        fn now_ms(&self) -> u64 {
            svc_call(abi::NOW_MS, 0, 0, 0, 0) as u64
        }

        fn sleep_ms(&mut self, ms: u32) {
            let _ = svc_call(abi::SLEEP_MS, ms, 0, 0, 0);
        }

        fn gpio_write(&mut self, _pin: GpioPin, high: bool) {
            let _ = svc_call(abi::GPIO_WRITE, 0, high as u32, 0, 0);
        }

        fn gpio_toggle(&mut self, _pin: GpioPin) {
            let _ = svc_call(abi::GPIO_TOGGLE, 0, 0, 0, 0);
        }
    }
}

// ───────────── SCHEDULER & TASKS ─────────────

mod sched {
    use cortex_m_rt::exception;
    use core::arch::global_asm;
    use crate::{task0_entry, task1_entry, task2_entry};
    use rtt_target::rprintln;

    pub const N_TASKS: usize = 3;
    const TASK_STACK_WORDS: usize = 256;
    const KERNEL_STACK_WORDS: usize = 512;  // Kernel needs more stack for complex operations

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct Tcb {
        pub sp: u32,
    }

    #[repr(align(8))]
    #[derive(Copy, Clone)]
    struct TaskStack([u32; TASK_STACK_WORDS]);

    #[repr(align(8))]
    #[derive(Copy, Clone)]
    struct KernelStack([u32; KERNEL_STACK_WORDS]);

    static mut TCBS: [Tcb; N_TASKS] = [Tcb { sp: 0 }; N_TASKS];
    static mut TASK_STACKS: [TaskStack; N_TASKS] = [TaskStack([0; TASK_STACK_WORDS]); N_TASKS];
    static mut KERNEL_STACK: KernelStack = KernelStack([0; KERNEL_STACK_WORDS]);
    static mut CURR: usize = 0;

    const ICSR: *mut u32 = 0xE000_ED04 as *mut u32;
    const SHPR3: *mut u32 = 0xE000_ED20 as *mut u32;

    #[inline(always)]
    fn build_initial_psp(stack: &mut [u32], entry: usize) -> u32 {
        const SW: usize = 8;  // Software frame: r4-r11
        const HW: usize = 8;  // Hardware frame: r0-r3, r12, lr, pc, xpsr
        const GUARD: usize = 8; // Guard space

        let len = stack.len();
        let base = len - (SW + HW + GUARD);

        rprintln!("[STACK] Building stack for entry=0x{:08X}", entry);
        rprintln!("[STACK] Stack layout: SW({}) + HW({}) + GUARD({}) = {} words", SW, HW, GUARD, SW + HW + GUARD);
        rprintln!("[STACK] Stack length: {} words, base offset: {}", len, base);

        // Clear and initialize software context (r4-r11) to zero
        for i in 0..SW {
            stack[base + i] = 0;
        }
        rprintln!("[STACK] SW context (R4-R11) cleared at offset {}-{}", base, base + SW - 1);

        // Initialize hardware context
        let hw = base + SW;
        rprintln!("[STACK] HW context starts at offset {}", hw);

        stack[hw + 0] = 0x11111111;  // R0 - distinctive value for debugging
        stack[hw + 1] = 0x22222222;  // R1
        stack[hw + 2] = 0x33333333;  // R2
        stack[hw + 3] = 0x44444444;  // R3
        stack[hw + 4] = 0x55555555;  // R12
        stack[hw + 5] = (task_return_trap as u32) | 1;  // LR with Thumb bit
        let pc_value = (entry as u32 & !1) | 1;  // PC with Thumb bit
        stack[hw + 6] = pc_value;
        stack[hw + 7] = 0x0100_0000;  // xPSR with Thumb state

        // Debug the actual stack frame values
        rprintln!("[STACK] HW frame initialized:");
        rprintln!("[STACK]   R0={:08X} R1={:08X} R2={:08X} R3={:08X}", stack[hw+0], stack[hw+1], stack[hw+2], stack[hw+3]);
        rprintln!("[STACK]   R12={:08X} LR={:08X} PC={:08X} xPSR={:08X}", stack[hw+4], stack[hw+5], stack[hw+6], stack[hw+7]);

        // Calculate actual addresses
        let psp = unsafe { stack.as_ptr().add(base) as u32 };  // Points to SW context start
        let hw_context = unsafe { stack.as_ptr().add(hw) as u32 };  // Points to HW context start

        rprintln!("[STACK] Memory layout:");
        rprintln!("[STACK]   PSP (SW start) = 0x{:08X}", psp);
        rprintln!("[STACK]   HW start      = 0x{:08X} (PSP + {})", hw_context, hw_context - psp);
        rprintln!("[STACK]   Expected: PSP + 32 = 0x{:08X}", psp + 32);

        if hw_context != psp + 32 {
            rprintln!("[STACK] ERROR: HW context not at PSP+32!");
        }

        // Verify memory bounds
        let stack_start = stack.as_ptr() as u32;
        let stack_end = unsafe { stack.as_ptr().add(stack.len()) as u32 };
        rprintln!("[STACK] Stack bounds: 0x{:08X} - 0x{:08X} (size={})", stack_start, stack_end, stack.len() * 4);

        if psp < stack_start || psp >= stack_end {
            rprintln!("[STACK] ERROR: PSP 0x{:08X} out of bounds!", psp);
            loop {}
        }
        if hw_context < stack_start || hw_context >= stack_end {
            rprintln!("[STACK] ERROR: HW context 0x{:08X} out of bounds!", hw_context);
            loop {}
        }

        // Verify alignment
        if psp & 7 != 0 {
            rprintln!("[STACK] ERROR: PSP 0x{:08X} not 8-byte aligned!", psp);
            loop {}
        }

        rprintln!("[STACK] Stack initialization complete, returning PSP=0x{:08X}", psp);
        psp
    }

    extern "C" fn task_return_trap() -> ! {
        loop {
            unsafe { core::ptr::write_volatile(ICSR, 1 << 28); }
        }
    }

    pub unsafe fn init_kernel_and_tasks() {
        unsafe {
            rprintln!("[INIT] Setting up kernel stack...");

            // Initialize kernel stack - MSP will continue to use this
            let kernel_stack_ptr = core::ptr::addr_of_mut!(KERNEL_STACK.0);
            let kernel_stack_base = (*kernel_stack_ptr).as_ptr() as u32;
            let kernel_stack_top = kernel_stack_base + (KERNEL_STACK_WORDS * 4) as u32;
            rprintln!("[INIT] Kernel stack: 0x{:08X} - 0x{:08X} (size={} bytes)",
                     kernel_stack_base,
                     kernel_stack_top,
                     KERNEL_STACK_WORDS * 4);

            // MSP should already be pointing to a valid kernel stack
            // We don't change MSP here - it stays as the kernel/interrupt stack

            rprintln!("[INIT] Initializing {} tasks...", N_TASKS);

            let p0 = build_initial_psp(&mut TASK_STACKS[0].0, task0_entry as usize);
            let p1 = build_initial_psp(&mut TASK_STACKS[1].0, task1_entry as usize);
            let p2 = build_initial_psp(&mut TASK_STACKS[2].0, task2_entry as usize);

            TCBS[0].sp = p0;
            TCBS[1].sp = p1;
            TCBS[2].sp = p2;
            CURR = 0;

            rprintln!("[INIT] Kernel and tasks ready");
            rprintln!("[INIT] MSP (kernel): 0x{:08X}", cortex_m::register::msp::read() as u32);
            rprintln!("[INIT] PSP (unused): 0x{:08X}", cortex_m::register::psp::read() as u32);
        }
    }

    pub unsafe fn init_systick_50us() {
        let mut v = unsafe { core::ptr::read_volatile(SHPR3) };
        v &= !0xFFFF_0000;
        v |= (0x80u32 << 24) | (0xFFu32 << 16);
        unsafe { core::ptr::write_volatile(SHPR3, v); }

        let syst_csr = 0xE000_E010 as *mut u32;
        let syst_rvr = 0xE000_E014 as *mut u32;
        let syst_cvr = 0xE000_E018 as *mut u32;

        unsafe { core::ptr::write_volatile(syst_rvr, 799); }
        unsafe { core::ptr::write_volatile(syst_cvr, 0); }
        unsafe { core::ptr::write_volatile(syst_csr, (1 << 2) | (1 << 1) | 1); }
    }

    pub fn start() -> ! {
        rprintln!("[SCHED] Scheduler starting...");
        rprintln!("[SCHED] Kernel runs in privileged mode on MSP");
        rprintln!("[SCHED] Tasks will run in unprivileged mode on PSP");

        unsafe {
            rprintln!("[SCHED] Current MSP (kernel stack): 0x{:08X}", cortex_m::register::msp::read() as u32);
            rprintln!("[SCHED] Current PSP (task stack): 0x{:08X}", cortex_m::register::psp::read() as u32);
            rprintln!("[SCHED] Current CONTROL: 0x{:08X}", cortex_m::register::control::read().bits());

            rprintln!("[SCHED] Initializing SysTick for 50µs preemptive scheduling");
            init_systick_50us();

            rprintln!("[SCHED] Triggering first context switch to Task 0");
            rprintln!("[SCHED] After this PendSV, kernel returns to WFI but tasks execute on PSP");

            core::ptr::write_volatile(ICSR, 1 << 28);

            // The PendSV should switch to task 0, but kernel continues here
            rprintln!("[SCHED] PendSV completed - now in kernel idle loop");
            rprintln!("[SCHED] Task 0 should be running on PSP concurrently");
        }

        // Kernel idle loop - runs on MSP while tasks run on PSP
        rprintln!("[SCHED] Kernel entering WFI idle loop (MSP)");
        loop {
            cortex_m::asm::wfi();
        }
    }

    global_asm!(
        r#"
        .global PendSV
        .type PendSV, %function
    PendSV:
        @ PendSV always runs in privileged mode using MSP (kernel stack)
        @ This preserves kernel context automatically

        mrs     r0, psp
        cbz     r0, first_switch

    normal_switch:
        @ Normal task-to-task switch
        @ PSP points to current task's stack, save its context
        stmdb   r0!, {{r4-r11}}

        @ Call scheduler to get next task's SP
        bl      {switch}

        @ Load new task's context and set PSP
        ldmia   r0!, {{r4-r11}}
        msr     psp, r0
        bx      lr

    first_switch:
        @ Initial switch from kernel to first task
        @ PSP is 0, so we're switching from MSP (kernel) to PSP (task)

        bl      {switch}

        @ r0 contains the first task's SP pointing to software context
        @ Load task's software context (r4-r11)
        ldmia   r0!, {{r4-r11}}

        @ r0 now points to hardware context - this becomes new PSP
        msr     psp, r0

        @ Switch to unprivileged thread mode using PSP
        mrs     r1, CONTROL
        orr     r1, r1, #2     @ Use PSP for thread mode
        orr     r1, r1, #1     @ Switch to unprivileged mode
        msr     CONTROL, r1
        isb

        @ Set return to thread mode with PSP
        @ Hardware will restore r0-r3,r12,lr,pc,xpsr from PSP stack
        ldr     lr, =0xFFFFFFFD
        bx      lr
    "#,
        switch = sym pend_sv_switch_rust
    );


    pub extern "C" fn pend_sv_switch_rust(old_psp: u32) -> u32 {
        unsafe {
            let current_task = core::ptr::read_volatile(core::ptr::addr_of!(CURR));
            rprintln!("[PendSV] ENTRY: old_psp=0x{:08X}, curr={}", old_psp, current_task);

            if old_psp != 0 {
                rprintln!("[PendSV] Normal switch - saving old PSP");
                TCBS[current_task].sp = old_psp;
                let next_task = (current_task + 1) % N_TASKS;
                core::ptr::write_volatile(core::ptr::addr_of_mut!(CURR), next_task);
                rprintln!("[PendSV] Switch {} -> {}", current_task, next_task);
            } else {
                rprintln!("[PendSV] FIRST SWITCH - old_psp is 0, initializing task 0");
                rprintln!("[PendSV] Current TCBS[0].sp = 0x{:08X}", TCBS[0].sp);
            }

            let new_task = core::ptr::read_volatile(core::ptr::addr_of!(CURR));
            let new_sp = TCBS[new_task].sp;
            rprintln!("[PendSV] About to load task {} with SP=0x{:08X}", new_task, new_sp);

            // Verify the stack pointer is valid
            if new_sp < 0x2000_0000 || new_sp >= 0x2002_0000 {
                rprintln!("[PendSV] ERROR: Invalid SP address! SP=0x{:08X}", new_sp);
                loop {} // Hang on error
            }

            // Verify alignment
            if new_sp & 7 != 0 {
                rprintln!("[PendSV] ERROR: SP not 8-byte aligned! SP=0x{:08X}", new_sp);
                loop {} // Hang on error
            }

            // Check what's at the stack pointer location
            let sp_ptr = new_sp as *const u32;
            rprintln!("[PendSV] Stack contents at 0x{:08X}:", new_sp);
            for i in 0..12 {
                let val = core::ptr::read_volatile(sp_ptr.add(i));
                rprintln!("[PendSV]   [{}] = 0x{:08X}", i, val);
            }

            rprintln!("[PendSV] Assembly will do: add r0, r0, #32 -> PSP=0x{:08X}", new_sp + 32);
            rprintln!("[PendSV] This should point to HW frame (R0,R1,R2,R3,R12,LR,PC,xPSR)");

            let hw_frame_ptr = (new_sp + 32) as *const u32;
            rprintln!("[PendSV] HW frame contents at 0x{:08X}:", new_sp + 32);
            for i in 0..8 {
                let val = core::ptr::read_volatile(hw_frame_ptr.add(i));
                let reg_name = match i {
                    0 => "R0", 1 => "R1", 2 => "R2", 3 => "R3",
                    4 => "R12", 5 => "LR", 6 => "PC", 7 => "xPSR",
                    _ => "??"
                };
                rprintln!("[PendSV]   {} = 0x{:08X}", reg_name, val);
            }

            rprintln!("[PendSV] Returning SP=0x{:08X} to assembly", new_sp);
            new_sp
        }
    }

    #[exception]
    fn SysTick() {
        // rprintln!("[SysTick] interrupt fired");
        unsafe { core::ptr::write_volatile(ICSR, 1 << 28); }
    }
}


// ───────────── TASKS ─────────────

#[unsafe(no_mangle)]
pub extern "C" fn task0_entry() -> ! {
    // CRITICAL TEST: If we reach here, LED should turn on and stay on
    unsafe {
        // Add RTT logging first to confirm we actually reach this function
        use rtt_target::rprintln;
        rprintln!("[TASK0] *** TASK0_ENTRY CALLED SUCCESSFULLY! ***");
        rprintln!("[TASK0] About to turn on LED...");

        // Direct GPIO access to avoid any syscall issues
        use core::ptr::{write_volatile};
        const GPIOA_BASE: u32 = 0x4002_0000;
        const BSRR_OFF: u32 = 0x18;
        let bsrr = (GPIOA_BASE + BSRR_OFF) as *mut u32;
        write_volatile(bsrr, 1 << 5); // Turn on PA5 (LED)

        rprintln!("[TASK0] LED should be ON now!");
    }

    // Infinite loop to test if we actually get here
    let mut counter = 0u32;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 16_000_000 == 0 {
            use rtt_target::rprintln;
            rprintln!("[TASK0] Still alive... counter={}", counter);
        }
        // Do nothing - just keep LED on
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn task1_entry() -> ! {
    loop {
        // Task1 doesn't control LED to avoid conflict with Task0
        syscalls().sleep_ms(1000);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn task2_entry() -> ! {
    static mut LAST: u64 = 0;
    loop {
        let _now = syscalls().now_ms();
        unsafe {
            LAST = _now;
        }
        for _ in 0..1200 {
            cortex_m::asm::nop();
        }
    }
}

// ───────────── MAIN ENTRY ─────────────
static mut BOARD: Option<board::BoardSyscalls> = None;
static mut SYSCALL_CLIENT: Option<svc::Client> = None;
static mut SYSCALLS_PTR: *mut svc::Client = core::ptr::null_mut();

#[inline(always)]
fn syscalls() -> &'static mut svc::Client {
    unsafe {
        if SYSCALLS_PTR.is_null() {
            // Hang instead of using rprintln
            loop {}
        }
        &mut *SYSCALLS_PTR
    }
}

#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("[mini-os] Booting");

    unsafe {
        // ───── Initialize static BOARD instance ─────
        BOARD = Some(board::BoardSyscalls::new(
            board::RawPin::new(board::GPIOA, 5),
            board::RawPin::new(board::GPIOC, 13),
            CYCLES_PER_MS_ESTIMATE,
        ));

        // Extract raw pointer to BOARD
        let board_option_ptr = core::ptr::addr_of_mut!(BOARD);
        let board_ptr: *mut board::BoardSyscalls = (*board_option_ptr).as_mut().unwrap() as *mut _;

        // Call .init() via pointer deref
        (*board_ptr).init();

        // Register with SVC
        svc::register_kernel_board(board_ptr);

        // ───── Create syscall client instance and assign to global ─────
        SYSCALL_CLIENT = Some(svc::Client::new(&mut *board_ptr));
        let client_option_ptr = core::ptr::addr_of_mut!(SYSCALL_CLIENT);
        SYSCALLS_PTR = (*client_option_ptr).as_mut().unwrap() as *mut _;
    }

    // LED test removed - now only Task0 will control the LED

    rprintln!("[MAIN] About to initialize kernel and tasks");
    unsafe {
        sched::init_kernel_and_tasks();
        rprintln!("[MAIN] Kernel and tasks initialized");
    }

    rprintln!("[MAIN] About to start scheduler");
    sched::start();
}
