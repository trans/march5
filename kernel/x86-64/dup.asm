; dup ( a -- a a )
; Duplicates the top stack item
; Stack effect: Push copy of TOS

section .text
extern vm_dispatch
global op_dup

op_dup:
    ; rsi = data stack pointer (grows downward)
    ; Stack layout: [TOS] <- rsi points here

    mov rax, [rsi]          ; Load TOS
    sub rsi, 8              ; Allocate space for new item
    mov [rsi], rax          ; Push duplicate
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
