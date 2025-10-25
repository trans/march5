; execute ( addr -- )
; Pops an address from the stack and executes it as a word
; The address should point to executable machine code (like a quotation)
; Stack effect: Pop address, execute word at that address

section .text
extern vm_dispatch
global op_execute

op_execute:
    ; rsi = data stack pointer (grows downward)
    ; rdi = return stack pointer (grows downward)

    ; Pop address from data stack
    mov rax, [rsi]          ; Load address from TOS
    add rsi, 8              ; Drop from data stack

    ; Check for null address (safety)
    test rax, rax
    jz .done                ; If null, just return

    ; Call the word at the address
    ; The word will manipulate stacks as needed and return to us
    call rax

.done:
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
