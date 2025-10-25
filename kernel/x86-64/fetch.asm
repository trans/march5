; @ ( addr -- value )
; Fetches a 64-bit value from memory address
; Stack effect: Pop address, push value at that address

section .text
extern vm_dispatch
global op_fetch

op_fetch:
    ; rsi = data stack pointer
    ; [rsi] = TOS (addr)

    mov rax, [rsi]          ; Load address
    mov rax, [rax]          ; Fetch value from that address
    mov [rsi], rax          ; Store value on stack
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
