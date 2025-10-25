; 0< ( n -- flag )
; Tests if top of stack is less than zero
; Stack effect: Pop one, push -1 (true) if n < 0, 0 (false) otherwise

section .text
extern vm_dispatch
global op_zerolt

op_zerolt:
    ; rsi = data stack pointer
    ; [rsi] = TOS (n)

    mov rax, [rsi]          ; Load n
    test rax, rax           ; Compare with zero
    setl al                 ; Set al to 1 if n < 0 (signed)
    movzx rax, al           ; Zero-extend to 64-bit
    neg rax                 ; Convert 1 to -1
    mov [rsi], rax          ; Store flag
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
