; >r ( n -- ) ( R: -- n )
; Moves top of data stack to return stack
; Stack effect: Pop from data stack, push to return stack

section .text
extern vm_dispatch
global op_tor

op_tor:
    ; rsi = data stack pointer
    ; rdi = return stack pointer (grows downward)
    ; [rsi] = TOS (n)

    mov rax, [rsi]          ; Load value from data stack
    add rsi, 8              ; Drop from data stack
    sub rdi, 8              ; Allocate space on return stack
    mov [rdi], rax          ; Push to return stack
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
