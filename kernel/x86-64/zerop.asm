; 0= ( n -- flag )
; Tests if top of stack equals zero
; Stack effect: Pop one, push -1 (true) if zero, 0 (false) otherwise

section .text
extern vm_dispatch
global op_zerop

op_zerop:
    ; rsi = data stack pointer
    ; [rsi] = TOS (n)

    mov rax, [rsi]          ; Load n
    test rax, rax           ; Test if zero
    setz al                 ; Set al to 1 if zero, 0 otherwise
    movzx rax, al           ; Zero-extend to 64-bit
    neg rax                 ; Convert 1 to -1
    mov [rsi], rax          ; Store flag
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
