; lor ( flag1 flag2 -- result )
; Logical OR - returns true if either flag is non-zero
; Stack effect: Pop two, push -1 (true) or 0 (false)

section .text
extern vm_dispatch
global op_lor

op_lor:
    ; rsi = data stack pointer
    ; [rsi] = TOS (flag2)
    ; [rsi+8] = second (flag1)

    mov rax, [rsi + 8]      ; Load flag1
    or rax, [rsi]           ; OR with flag2
    test rax, rax           ; Test if result is non-zero
    jz .false               ; Jump if zero

.true:
    mov rax, -1             ; At least one non-zero: true
    jmp .done

.false:
    xor rax, rax            ; Both zero: false

.done:
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
