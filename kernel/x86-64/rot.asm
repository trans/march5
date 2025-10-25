; rot ( a b c -- b c a )
; Rotates the top three stack items
; Stack effect: Third item moves to top

section .text
extern vm_dispatch
global op_rot

op_rot:
    ; rsi = data stack pointer
    ; [rsi] = TOS (c)
    ; [rsi+8] = second (b)
    ; [rsi+16] = third (a)

    mov rax, [rsi]          ; Load c
    mov rbx, [rsi + 8]      ; Load b
    mov rcx, [rsi + 16]     ; Load a

    mov [rsi], rcx          ; Store a at TOS
    mov [rsi + 8], rax      ; Store c at second
    mov [rsi + 16], rbx     ; Store b at third
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
