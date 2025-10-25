; mod ( a b -- remainder )
; Computes remainder of a / b
; Stack effect: Pop two, push remainder

section .text
extern vm_dispatch
global op_mod

op_mod:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b - divisor)
    ; [rsi+8] = second (a - dividend)

    mov rax, [rsi + 8]      ; Load dividend (a)
    cqo                     ; Sign-extend rax into rdx:rax
    idiv qword [rsi]        ; Signed divide by b, remainder in rdx
    add rsi, 8              ; Drop one item
    mov [rsi], rdx          ; Store remainder
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
