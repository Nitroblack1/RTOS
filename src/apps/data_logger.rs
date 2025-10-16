//! Data Logger Application
//! 🚀 11번째 앱 - 진짜 링크 타임 디스커버리 검증용!
//! 이 앱은 어떤 수동 등록 함수 호출 없이 순수하게 register_app! 매크로만으로 등록됨

use crate::app_syscalls;

// ✨ 진짜 자동 등록! 이 한 줄로 앱이 시스템에 자동으로 추가됨
crate::register_app!(data_logger_entry, 10, "data_logger", 384);

#[unsafe(no_mangle)]
pub extern "C" fn data_logger_entry() -> ! {
    app_syscalls::debug_print(10, "📊 Data Logger app 시작!");

    let mut log_count = 0u32;

    loop {
        // 데이터 로깅 시뮬레이션
        log_count = log_count.wrapping_add(1);

        if log_count % 25 == 0 {
            app_syscalls::debug_print(10, "📊 센서 데이터 로깅 중...");
        }

        if log_count % 100 == 0 {
            app_syscalls::debug_print(10, "📊 로그 파일 저장 완료");
        }

        // 다른 앱들에게 CPU 양보
        app_syscalls::yield_cpu();

        // 로깅 간격
        for _ in 0..20000 {
            cortex_m::asm::nop();
        }
    }
}