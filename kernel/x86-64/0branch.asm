; March VM - 0branch primitive
; Conditional branch: if TOS=0, read next cell and branch, else skip next cell
;
; Stack effect: ( flag -- )
; If flag is 0, branches. Otherwise continues.

section .text
    global op_0branch
    extern vm_dispatch

op_0branch:
    ; rsi = data stack pointer
    ; rdi = return stack pointer (TOS has saved IP)
    ; rbx = IP (already advanced past XT of 0branch)

    ; Pop flag from data stack
    mov rax, [rsi]              ; Load TOS (flag)
    add rsi, 8                  ; Pop from stack

    ; Test if flag is zero
    test rax, rax
    jnz .skip_branch            ; Non-zero: skip the offset, don't branch

.do_branch:
    ; Flag is zero: read offset and branch
    mov rax, [rbx]              ; Load offset cell (LIT)
    add rbx, 8                  ; Advance IP past offset

    ; Extract offset from LIT
    sar rax, 2                  ; Sign-extend from 62-bit LIT

    ; Adjust IP by offset (in cells)
    shl rax, 3                  ; offset * 8
    add rbx, rax                ; IP += offset * 8

    ; Update saved IP on return stack (VM will restore it)
    mov [rdi], rbx

    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)

.skip_branch:
    ; Flag is non-zero: just skip the offset cell
    add rbx, 8                  ; Skip offset LIT

    ; Update saved IP on return stack (VM will restore it)
    mov [rdi], rbx

    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
