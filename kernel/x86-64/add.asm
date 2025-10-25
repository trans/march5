; + ( a b -- sum )
; Adds the top two stack items
; Stack effect: Pop two, push sum

section .text
global op_add
extern vm_dispatch

op_add:
    ; rsi = data stack pointer
    ; [rsi] = TOS (b)
    ; [rsi+8] = second (a)

    mov rax, [rsi + 8]      ; Load a
    add rax, [rsi]          ; Add b
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
