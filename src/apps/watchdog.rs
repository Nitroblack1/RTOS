//! Watchdog Monitoring Application
//! 진짜 링크 타임 디스커버리 테스트용 새 앱

use crate::app_syscalls;

// ✨ 진짜 자동 등록! 이 한 줄로 앱이 시스템에 자동으로 추가됨
crate::register_app!(watchdog_entry, 8, "watchdog", 256);

#[unsafe(no_mangle)]
pub extern "C" fn watchdog_entry() -> ! {
    app_syscalls::debug_print(8, "🐕 Watchdog app 시작!");

    let mut heartbeat_count = 0u32;

    loop {
        // 시스템 상태 모니터링
        heartbeat_count = heartbeat_count.wrapping_add(1);

        if heartbeat_count % 20 == 0 {
            app_syscalls::debug_print(8, "💓 시스템 정상, Watchdog 체크 완료");
        }

        // 다른 앱들에게 CPU 양보
        app_syscalls::yield_cpu();

        // 하트비트 간격
        for _ in 0..30000 {
            cortex_m::asm::nop();
        }
    }
}