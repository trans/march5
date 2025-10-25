; March VM - Inner Interpreter
; Executes pre-compiled cell streams with tagged dispatch
;
; Register allocation:
;   rsi = Data stack pointer (grows down)
;   rdi = Return stack pointer (grows down)
;   rbx = Instruction pointer (IP) - points to current cell
;   rbp = VM context pointer (preserved)
;
; Cell encoding (64-bit) - Variable-bit tags:
;   2-bit tags:
;     00  = XT   (execute word, if addr=0 then EXIT)
;     01  = LIT  (immediate 62-bit literal)
;     10  = LST  (symbol ID literal)
;   3-bit tags (when low 2 bits = 11):
;     110 = LNT  (next N cells are raw literals)
;     111 = EXT  (future extension)

section .data
    align 8
    vm_running: dq 0        ; Flag: 1 if VM is running

section .bss
    align 16
    data_stack_base: resq 1024      ; Data stack (8KB)
    return_stack_base: resq 1024    ; Return stack (8KB)

section .text
    global vm_init
    global vm_run
    global vm_halt
    global vm_get_dsp
    global vm_get_rsp
    global vm_dispatch          ; Export dispatch loop for DOCOL
    global data_stack_base

; ============================================================================
; vm_init - Initialize the VM
; C signature: void vm_init(void)
; ============================================================================
vm_init:
    push rbp
    mov rbp, rsp

    ; Initialize data stack pointer (top of stack)
    lea rax, [rel data_stack_base]
    add rax, 8 * 1024           ; Point to end of stack area
    sub rax, 8                  ; Back up one slot
    mov [rel data_stack_top], rax

    ; Initialize return stack pointer
    lea rax, [rel return_stack_base]
    add rax, 8 * 1024
    sub rax, 8
    mov [rel return_stack_top], rax

    ; Clear running flag
    mov qword [rel vm_running], 0

    pop rbp
    ret

; ============================================================================
; vm_run - Execute a cell stream
; C signature: void vm_run(uint64_t* code_ptr)
; Arguments:
;   rdi = pointer to first cell of code stream
; ============================================================================
vm_run:
    push rbp
    mov rbp, rsp

    ; Save callee-saved registers
    push rbx
    push r12
    push r13
    push r14
    push r15

    ; Set up VM registers
    mov rbx, rdi                        ; IP = code pointer (arg)
    mov rsi, [rel data_stack_top]       ; DSP = data stack top
    mov rdi, [rel return_stack_top]     ; RSP = return stack top

    ; Mark VM as running
    mov qword [rel vm_running], 1

    ; Fall through to dispatch loop

; ============================================================================
; Inner Interpreter - Main dispatch loop
; ============================================================================
vm_dispatch:
    ; Check if VM should halt
    mov rax, [rel vm_running]
    test rax, rax
    jz .halt

    ; Fetch next cell
    mov rcx, [rbx]              ; Load cell
    add rbx, 8                  ; Advance IP

    ; Decode variable-bit tag
    mov rax, rcx
    and rax, 0x3                ; Get low 2 bits

    ; Dispatch on low 2 bits
    cmp rax, 0
    je .do_xt                   ; 00 = XT
    cmp rax, 1
    je .do_lit                  ; 01 = LIT
    cmp rax, 2
    je .decode_10               ; 10 = LST or LNT (check bit 2)
    jmp .do_ext                 ; 11 = EXT

.decode_10:
    ; Low 2 bits are 10, check bit 2 to distinguish LST from LNT
    test rcx, 0x4               ; Check bit 2
    jz .do_lst                  ; 010 = LST
    jmp .do_lnt                 ; 110 = LNT

; ----------------------------------------------------------------------------
; XT (00) - Execute word at address (or EXIT if addr=0)
; ----------------------------------------------------------------------------
.do_xt:
    ; Clear tag bits to get address
    and rcx, ~0x3               ; Mask off low 2 bits

    ; Check for EXIT (address 0)
    test rcx, rcx
    jz .do_exit

    ; Direct threading: just jump to the address
    ; Both primitives and user words (DOCOL) will jmp back to vm_dispatch
    jmp rcx

; ----------------------------------------------------------------------------
; EXIT - Return from word (when XT addr=0)
; ----------------------------------------------------------------------------
.do_exit:
    ; Check if return stack is at base (we're done)
    lea rax, [rel return_stack_base]
    add rax, 8 * 1024
    sub rax, 8
    cmp rdi, rax
    jge .halt                   ; Return stack empty, halt VM

    ; Pop IP from return stack
    mov rbx, [rdi]              ; Load saved IP
    add rdi, 8                  ; Drop from return stack

    jmp vm_dispatch

; ----------------------------------------------------------------------------
; LIT (01) - Immediate 62-bit literal
; ----------------------------------------------------------------------------
.do_lit:
    ; Value is embedded in cell (upper 62 bits)
    mov rax, rcx
    sar rax, 2                  ; Sign-extend from 62 bits

    ; Push to data stack
    sub rsi, 8                  ; Allocate space
    mov [rsi], rax              ; Store literal

    jmp vm_dispatch

; ----------------------------------------------------------------------------
; LST (10) - Symbol literal
; ----------------------------------------------------------------------------
.do_lst:
    ; Symbol ID is in upper 62 bits (unsigned)
    mov rax, rcx
    shr rax, 2                  ; Unsigned shift

    ; Push to data stack
    sub rsi, 8
    mov [rsi], rax

    jmp vm_dispatch

; ----------------------------------------------------------------------------
; LNT (110) - Next N cells are raw literals
; ----------------------------------------------------------------------------
.do_lnt:
    ; Get count from upper 61 bits
    mov rax, rcx
    shr rax, 3                  ; Extract count (61 bits)
    mov rdx, rax                ; rdx = counter

.lnt_loop:
    test rdx, rdx
    jz vm_dispatch              ; Done with literals

    mov rax, [rbx]              ; Load literal value
    add rbx, 8                  ; Advance IP

    sub rsi, 8                  ; Allocate stack space
    mov [rsi], rax              ; Push to stack

    dec rdx
    jmp .lnt_loop

; ----------------------------------------------------------------------------
; 011 and 111 tags - Reserved for future use
; ----------------------------------------------------------------------------
.do_ext:
    ; For now, just skip (NOP)
    jmp vm_dispatch

; ----------------------------------------------------------------------------
; Error handling
; ----------------------------------------------------------------------------
.error:
    ; Invalid tag - halt VM
    mov qword [rel vm_running], 0
    ; Fall through to halt

; ----------------------------------------------------------------------------
; Halt VM and return to caller
; ----------------------------------------------------------------------------
.halt:
    ; Mark VM as stopped
    mov qword [rel vm_running], 0

    ; Save final stack pointers
    mov [rel data_stack_top], rsi
    mov [rel return_stack_top], rdi

    ; Restore callee-saved registers
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx

    pop rbp
    ret

; ============================================================================
; vm_halt - Stop the VM
; C signature: void vm_halt(void)
; ============================================================================
vm_halt:
    mov qword [rel vm_running], 0
    ret

; ============================================================================
; vm_get_dsp - Get current data stack pointer
; C signature: uint64_t* vm_get_dsp(void)
; ============================================================================
vm_get_dsp:
    mov rax, [rel data_stack_top]
    ret

; ============================================================================
; vm_get_rsp - Get current return stack pointer
; C signature: uint64_t* vm_get_rsp(void)
; ============================================================================
vm_get_rsp:
    mov rax, [rel return_stack_top]
    ret

; ============================================================================
; Data section for stack tops
; ============================================================================
section .data
    align 8
    data_stack_top: dq 0
    return_stack_top: dq 0
