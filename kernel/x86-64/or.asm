; or ( a b -- result )
; Bitwise OR of top two stack items
; Stack effect: Pop two, push bitwise OR

section .text
extern vm_dispatch
global op_or

op_or:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    or rax, [rsi]           ; Bitwise OR with b
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
