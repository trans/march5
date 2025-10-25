; << ( value count -- result )
; Logical left shift
; Stack effect: Pop value and count, push value << count

section .text
extern vm_dispatch
global op_lshift

op_lshift:
    ; rsi = data stack pointer
    ; [rsi] = TOS (count)
    ; [rsi+8] = second (value)

    mov rax, [rsi + 8]      ; Load value
    mov rcx, [rsi]          ; Load count into cl (shift amount)
    shl rax, cl             ; Logical left shift
    add rsi, 8              ; Drop one item
    mov [rsi], rax          ; Store result
    jmp vm_dispatch         ; Return to VM dispatch (FORTH-style)
