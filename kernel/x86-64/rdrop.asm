; rdrop ( -- ) ( R: n -- )
; Removes top item from return stack
; Stack effect: Pop and discard return stack TOS

section .text
extern vm_dispatch
global op_rdrop

op_rdrop:
    ; rdi = return stack pointer

    add rdi, 8              ; Drop TOS from return stack
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
