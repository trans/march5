; lnot ( flag -- result )
; Logical NOT - inverts boolean flag
; Stack effect: Pop one, push inverted flag
; Note: 0 -> -1 (true), non-zero -> 0 (false)

section .text
extern vm_dispatch
global op_lnot

op_lnot:
    ; rsi = data stack pointer
    ; [rsi] = TOS (flag)

    mov rax, [rsi]          ; Load flag
    test rax, rax           ; Test if zero
    jz .true                ; If zero, return true

.false:
    xor rax, rax            ; Non-zero input: return false
    jmp .done

.true:
    mov rax, -1             ; Zero input: return true

.done:
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
