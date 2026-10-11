; Runtime bytecode 0x6000545000, hand-assembled without a compiler.
; Reads the shared slot 0, discards its value, and succeeds.
PUSH1 0x00
SLOAD
POP
STOP
