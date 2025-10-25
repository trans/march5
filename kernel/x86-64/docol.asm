; DOCOL - Do Colon Definition
; Runtime entry for user-defined words (direct threading)
;
; Entry conditions (from VM's jmp):
;   rsi = data stack pointer
;   rdi = return stack pointer
;   rbx = IP (pointing to next cell after the XT)
;
; The cell stream pointer is passed in rax by the wrapper

section .text
global docol
extern vm_dispatch

docol:
    ; rax contains the address of the cell stream to execute

    ; Save current IP on return stack (for EXIT to restore)
    sub rdi, 8
    mov [rdi], rbx

    ; Set IP to the cell stream
    mov rbx, rax

    ; Jump to VM dispatch loop to start executing the cells
    jmp vm_dispatch
