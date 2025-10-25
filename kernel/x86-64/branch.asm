; March VM - branch primitive
; Unconditional branch: read next cell as offset and adjust IP
;
; Stack effect: ( -- )
; Reads next cell (LIT) as signed offset in cells, adjusts IP

section .text
    global op_branch
    extern vm_dispatch

op_branch:
    ; rdi = return stack pointer (TOS has saved IP)
    ; rbx = IP (already advanced past XT of branch)

    ; Read next cell as LIT offset
    mov rax, [rbx]              ; Load offset cell
    add rbx, 8                  ; Advance IP past offset

    ; Extract offset from LIT (shift right 2 bits for signed value)
    sar rax, 2                  ; Sign-extend from 62-bit LIT

    ; Adjust IP by offset (in cells, convert to bytes)
    shl rax, 3                  ; offset * 8
    add rbx, rax                ; IP += offset * 8

    ; Update saved IP on return stack (VM will restore it)
    mov [rdi], rbx

    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
