; r> ( -- n ) ( R: n -- )
; Moves top of return stack to data stack
; Stack effect: Pop from return stack, push to data stack

section .text
extern vm_dispatch
global op_fromr

op_fromr:
    ; rsi = data stack pointer
    ; rdi = return stack pointer
    ; [rdi] = return stack TOS (n)

    mov rax, [rdi]          ; Load value from return stack
    add rdi, 8              ; Drop from return stack
    sub rsi, 8              ; Allocate space on data stack
    mov [rsi], rax          ; Push to data stack
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
