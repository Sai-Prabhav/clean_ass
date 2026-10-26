default rel
section .data
    msg:    db "Hello, world! "    ; message + newline (10 = '\n')
    msglen: equ $ - msg               ; length computed from addresses

section .bss
    chr_buf resb 100
    count resb 1

section .text
    global _start

print:
    dec     r12
    mov     rax, 1          ; sys_write
    mov     rdi, 1          ; fd 1 = stdout
    mov     rsi, msg        ; buffer address
    mov     rdx, msglen     ; number of bytes
    syscall
    mov     r14, r12        ; r14 is runnig var for printing
    sub     r14, [count]
    neg     r14
    mov     r13, 10         ; base 10
    mov     r15, chr_buf    ; stores the pointer
    mov     byte [chr_buf], 10   ; end with \n
    inc     r15             ; start after \n
itos:
    mov     rax, r14        ; div needs rax
    div     r13b            ; q in al, r in ah
    mov     r14b, al        ; copy al to runnig var
    add     ah,"0"          ; convert digit to chr
    mov     al, ah          ; cant move ah to 
    mov     byte [r15], al
    inc     r15             ; increment pointer
    cmp     r14b, 0
    jne     itos

; chars are from r15 to chr_buf
revbuf:
    mov     rax, 1          ; sys_write
    mov     rdi, 1          ; fd 1 = stdout
    mov     rdx, 1          ; number of bytes
    mov     rsi, r15        ; what to put
    syscall

    dec     r15
    cmp     r15, chr_buf-1
    jne     revbuf
    cmp     r12, 0
    jne     print
    ret 
_start:
    ; write(1, msg, msglen)
    mov     byte [count], 100
    mov     r12, [count]
    call    print
    syscall

    ; exit(0)
exit:
    mov     rax, 60         ; sys_exit
    xor     edi, edi        ; status = 0
    syscall
