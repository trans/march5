; c@ ( addr -- byte )
; Fetches a byte from memory address (zero-extended to 64-bit)
; Stack effect: Pop address, push byte value

section .text
extern vm_dispatch
global op_cfetch

op_cfetch:
    ; rsi = data stack pointer
    ; [rsi] = TOS (addr)

    mov rax, [rsi]          ; Load address
    movzx rax, byte [rax]   ; Fetch byte, zero-extend to 64-bit
    mov [rsi], rax          ; Store on stack
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
