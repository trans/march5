; <= ( a b -- flag )
; Tests if a <= b (signed comparison)
; Stack effect: Pop two, push -1 (true) or 0 (false)

section .text
extern vm_dispatch
global op_le

op_le:
    ; rsi = data stack pointer

    mov rax, [rsi + 8]      ; Load a
    cmp rax, [rsi]          ; Compare a with b
    setle al                ; Set al to 1 if a <= b (signed)
    movzx rax, al           ; Zero-extend to 64-bit
    neg rax                 ; Convert 1 to -1
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store flag
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
