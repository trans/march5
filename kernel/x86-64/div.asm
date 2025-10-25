; / ( a b -- quotient )
; Divides a by b (a / b)
; Stack effect: Pop two, push quotient
; Note: Remainder is discarded (use /mod for both)

section .text
extern vm_dispatch
global op_div

op_div:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b - divisor)
    ; [rsi+8] = second (a - dividend)

    mov rax, [rsi + 8]      ; Load dividend (a)
    cqo                     ; Sign-extend rax into rdx:rax
    idiv qword [rsi]        ; Signed divide by b, quotient in rax
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store quotient
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
