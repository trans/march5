; xor ( a b -- result )
; Bitwise XOR of top two stack items
; Stack effect: Pop two, push bitwise XOR

section .text
extern vm_dispatch
global op_xor

op_xor:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    xor rax, [rsi]          ; Bitwise XOR with b
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
