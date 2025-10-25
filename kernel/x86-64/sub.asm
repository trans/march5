; - ( a b -- difference )
; Subtracts b from a (a - b)
; Stack effect: Pop two, push difference

section .text
extern vm_dispatch
global op_sub

op_sub:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    sub rax, [rsi]          ; Subtract b
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
