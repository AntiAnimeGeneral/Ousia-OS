ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);
  rodata PT_LOAD FLAGS(4);
  data PT_LOAD FLAGS(6);
}

SECTIONS
{
  . = 0x40080000;

  .text : ALIGN(4K) {
    KEEP(*(.text.boot))
    *(.text .text.*)
  } :text

  .rodata : ALIGN(4K) {
    *(.rodata .rodata.*)
  } :rodata

  .data : ALIGN(4K) {
    *(.data .data.*)
  } :data

  .bss : ALIGN(4K) {
    __bss_start = .;
    *(.bss .bss.*)
    *(COMMON)
    __bss_end = .;
  } :data

  /DISCARD/ : {
    *(.comment)
    *(.eh_frame*)
    *(.note*)
  }
}
