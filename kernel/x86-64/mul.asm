; * ( a b -- product )
; Multiplies the top two stack items
; Stack effect: Pop two, push product

section .text
extern vm_dispatch
global op_mul

op_mul:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    imul rax, [rsi]         ; Multiply by b (signed)
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
