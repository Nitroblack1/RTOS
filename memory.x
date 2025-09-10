MEMORY
{
  /* FLASH: 512K(446RE) 또는 1M(446RG) - 보드에 맞게 선택 */
  FLASH  (rx)  : ORIGIN = 0x08000000, LENGTH = 512K
  /* SRAM: 128K + CCM: 64K (필요시 CCM 사용) */
  RAM    (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
  /* 주로 DTCM/CCM RAM. FPU/연산용 버퍼 등 속도 필요시 사용 */
  CCRAM  (rwx) : ORIGIN = 0x10000000, LENGTH = 64K
}

/* 필요시 .ccram 섹션 매핑을 위해 아래를 링커 스크립트에 추가로 쓸 수 있음
SECTIONS
{
  .ccram (NOLOAD) : {
    *(.ccram .ccram.*);
  } > CCRAM
}
*/
