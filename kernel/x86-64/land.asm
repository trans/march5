; land ( flag1 flag2 -- result )
; Logical AND - returns true if both flags are non-zero
; Stack effect: Pop two, push -1 (true) or 0 (false)

section .text
extern vm_dispatch
global op_land

op_land:
    ; rsi = data stack pointer
    ; [rsi] = TOS (flag2)
    ; [rsi+8] = second (flag1)

    mov rax, [rsi + 8]      ; Load flag1
    test rax, rax           ; Test if flag1 is non-zero
    jz .false               ; Jump if zero

    mov rax, [rsi]          ; Load flag2
    test rax, rax           ; Test if flag2 is non-zero
    jz .false               ; Jump if zero

.true:
    mov rax, -1             ; Both non-zero: true
    jmp .done

.false:
    xor rax, rax            ; At least one zero: false

.done:
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
