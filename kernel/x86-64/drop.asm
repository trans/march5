; drop ( a -- )
; Removes the top stack item
; Stack effect: Pop and discard TOS

section .text
extern vm_dispatch
global op_drop

op_drop:
    ; rsi = data stack pointer

    add rsi, 8              ; Drop TOS by moving pointer up
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
