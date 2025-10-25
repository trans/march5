; and ( a b -- result )
; Bitwise AND of top two stack items
; Stack effect: Pop two, push bitwise AND

section .text
extern vm_dispatch
global op_and

op_and:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    and rax, [rsi]          ; Bitwise AND with b
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
