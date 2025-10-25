; 2>r ( n1 n2 -- ) ( R: -- n1 n2 )
; Moves top two items from data stack to return stack
; Stack effect: Pop two from data stack, push both to return stack
; Note: n2 is TOS on both stacks

section .text
extern vm_dispatch
global op_twotor

op_twotor:
    ; rsi = data stack pointer
    ; rdi = return stack pointer
    ; [rsi] = TOS (n2)
    ; [rsi+8] = second (n1)

    mov rax, [rsi + 8]      ; Load n1
    mov rbx, [rsi]          ; Load n2
    add rsi, 16             ; Drop both from data stack
    sub rdi, 16             ; Allocate space on return stack
    mov [rdi + 8], rax      ; Push n1 (will be second on return stack)
    mov [rdi], rbx          ; Push n2 (will be TOS on return stack)
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
