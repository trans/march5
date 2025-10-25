; >>> ( value count -- result )
; Arithmetic right shift (sign-extend)
; Stack effect: Pop value and count, push value >>> count

section .text
extern vm_dispatch
global op_arshift

op_arshift:
    ; rsi = data stack pointer
    ; [rsi] = TOS (count)
    ; [rsi+8] = second (value)

    mov rax, [rsi + 8]      ; Load value
    mov rcx, [rsi]          ; Load count into cl
    sar rax, cl             ; Arithmetic right shift (sign-extend)
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
