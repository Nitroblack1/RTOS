MEMORY
{
  /* FLASH: 512K(446RE) 또는 1M(446RG) - 보드에 맞게 선택 */
  FLASH  (rx)  : ORIGIN = 0x08000000, LENGTH = 512K
  /* SRAM: 128K + CCM: 64K (필요시 CCM 사용) */
  RAM    (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
  /* 주로 DTCM/CCM RAM. FPU/연산용 버퍼 등 속도 필요시 사용 */
  CCRAM  (rwx) : ORIGIN = 0x10000000, LENGTH = 64K
}

/* App metadata and memory protection sections */
SECTIONS
{
  /* CCM RAM section for high-performance buffers */
  .ccram (NOLOAD) : {
    *(.ccram .ccram.*);
  } > CCRAM

  /* Per-application static stacks live in RAM */
  .app_stacks (NOLOAD) : {
    . = ALIGN(8);
    __app_stacks_start = .;
    KEEP(*(.app_stacks .app_stacks.*));
    . = ALIGN(8);
    __app_stacks_end = .;
  } > RAM

  /* 🚀 App registry section - temporarily disabled for dynamic demo */
  /* .app_registry : {
    . = ALIGN(4);
    __app_registry_start = .;
    KEEP(*(.app_registry));
    KEEP(*(.app_registry.*));
    . = ALIGN(4);
    __app_registry_end = .;
  } > FLASH */
}

/* 🚀 실용적 링크 타임 디스커버리: 링커가 자동으로 앱 메타데이터 수집함을 시뮬레이션 */
/* 실제 embedded 프로젝트에서는 더 복잡한 링커 섹션 관리가 필요하지만, */
/* 개념적으로는 이와 동일한 방식으로 작동합니다 */
