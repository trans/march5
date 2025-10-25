; not ( a -- result )
; Bitwise NOT (one's complement) of top stack item
; Stack effect: Pop one, push bitwise NOT

section .text
extern vm_dispatch
global op_not

op_not:
    ; rsi = data stack pointer
    ; [rsi] = TOS (a)

    mov rax, [rsi]          ; Load a
    not rax                 ; Bitwise NOT
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
