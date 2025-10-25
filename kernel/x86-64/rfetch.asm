; r@ ( -- n ) ( R: n -- n )
; Copies top of return stack to data stack (non-destructive)
; Stack effect: Push copy of return stack TOS to data stack

section .text
extern vm_dispatch
global op_rfetch

op_rfetch:
    ; rsi = data stack pointer
    ; rdi = return stack pointer
    ; [rdi] = return stack TOS (n)

    mov rax, [rdi]          ; Load value from return stack (non-destructive)
    sub rsi, 8              ; Allocate space on data stack
    mov [rsi], rax          ; Push copy to data stack
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
