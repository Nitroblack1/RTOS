//! Data Logger Application
//! 🚀 11번째 앱 - 진짜 링크 타임 디스커버리 검증용!
//! 이 앱은 어떤 수동 등록 없이 #[app] 매크로만으로 등록됨
//! 추가로 동적 태스크 스폰 기능을 데모하는 앱!

use crate::{app_syscalls, sched};
use app_macros::app;
use rtt_target::rprintln;

// 🚀 동적으로 생성될 태스크의 entry 함수
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dynamic_worker_task() -> ! {
    // RTT 안정화 지연
    for _ in 0..25000 {
        cortex_m::asm::nop();
    }
    rprintln!("[LOGGER] start");

    let mut work_count = 0u32;

    loop {
        work_count = work_count.wrapping_add(1);

        if work_count % 100 == 0 {
            rprintln!("[LOGGER] {}", work_count);
        }

        // 다른 태스크에게 CPU 양보
        app_syscalls::yield_cpu();

        // 작업 시뮬레이션
        for _ in 0..5000 {
            cortex_m::asm::nop();
        }
    }
}

#[app(id = 10, stack_size = 384, name = "data_logger")]
fn data_logger() -> ! {
    app_syscalls::debug_print(10, "📊 Data Logger app 시작!");

    let mut log_count = 0u32;
    let mut spawned_task = false;

    loop {
        // 데이터 로깅 시뮬레이션
        log_count = log_count.wrapping_add(1);

        if log_count % 25 == 0 {
            app_syscalls::debug_print(10, "📊 센서 데이터 로깅 중...");
        }

        if log_count % 100 == 0 {
            app_syscalls::debug_print(10, "📊 로그 파일 저장 완료");
        }

        // 🚀 50번째 루프에서 동적 태스크 스폰!
        if log_count == 50 && !spawned_task {
            app_syscalls::debug_print(10, "📊 🚀 동적 워커 태스크를 생성합니다...");

            match sched::spawn_simple_task(dynamic_worker_task, "dynamic_worker") {
                Ok(_task_id) => {
                    app_syscalls::debug_print(10, "📊 ✅ 동적 태스크 생성 성공!");
                    spawned_task = true;

                    // 스택 풀 사용량 출력
                    let (_used, _total) = sched::get_stack_pool_usage();
                    let _task_count = sched::get_task_count();
                    app_syscalls::debug_print(10, "📊 📈 스택 사용량 정보 조회 완료");
                    app_syscalls::debug_print(10, "📊 📈 현재 동적 워커 태스크가 실행 중입니다");
                },
                Err(_e) => {
                    app_syscalls::debug_print(10, "📊 ❌ 동적 태스크 생성 실패");
                }
            }
        }

        // 다른 앱들에게 CPU 양보
        app_syscalls::yield_cpu();

        // 로깅 간격
        for _ in 0..20000 {
            cortex_m::asm::nop();
        }
    }
}
