; over ( a b -- a b a )
; Copies the second stack item to top
; Stack effect: Push copy of second item

section .text
extern vm_dispatch
global op_over

op_over:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load second item (a)
    sub rsi, 8              ; Allocate space
    mov [rsi], rax          ; Push copy of a
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
