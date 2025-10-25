; = ( a b -- flag )
; Tests if a equals b
; Stack effect: Pop two, push -1 (true) or 0 (false)
; Convention: -1 (all bits set) = true, 0 = false

section .text
extern vm_dispatch
global op_eq

op_eq:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    cmp rax, [rsi]          ; Compare with b
    sete al                 ; Set al to 1 if equal, 0 otherwise
    movzx rax, al           ; Zero-extend to 64-bit
    neg rax                 ; Convert 1 to -1 (0xFFFFFFFFFFFFFFFF)
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store flag
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
