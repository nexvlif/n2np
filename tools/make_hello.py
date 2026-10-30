"""Generate examples/hello/hello.exe: a console PE that writes to stdout via kernel32."""

import struct

from pebuild import PEBuilder, IMAGE_SUBSYSTEM_WINDOWS_CUI, SECTION_RVA, call_iat, lea_rip

CODE_RVA = 0x1000
TEXT_RVA = 0x1100
WRITTEN_RVA = 0x1180

message = b"hello from n2np\n\0"

imports = {
    "kernel32.dll": ["GetStdHandle", "WriteFile", "ExitProcess"],
}

builder = PEBuilder(IMAGE_SUBSYSTEM_WINDOWS_CUI, CODE_RVA, section_size=0x3000)
iat = builder.add_imports(imports)

builder.put(TEXT_RVA, message)
builder.put(WRITTEN_RVA, struct.pack("<I", 0))

code = bytearray()
code += b"\x48\x83\xEC\x28"  # sub rsp, 0x28
code += b"\xB9\xF5\xFF\xFF\xFF"  # mov ecx, STD_OUTPUT_HANDLE (-11)
call_iat(code, SECTION_RVA, iat["GetStdHandle"])
code += b"\x48\x89\xC3"  # mov rbx, rax
code += b"\x48\x89\xD9"  # mov rcx, rbx
lea_rip(code, SECTION_RVA, 0x48, 0x15, TEXT_RVA)  # lea rdx, [rip+message]
code += b"\x41\xB8" + struct.pack("<I", len(message) - 1)  # mov r8d, length
lea_rip(code, SECTION_RVA, 0x4C, 0x0D, WRITTEN_RVA)  # lea r9, [rip+written]
code += b"\x48\xC7\x44\x24\x20\x00\x00\x00\x00"  # mov qword [rsp+0x20], 0
call_iat(code, SECTION_RVA, iat["WriteFile"])
code += b"\x31\xC9"  # xor ecx, ecx
call_iat(code, SECTION_RVA, iat["ExitProcess"])
code += b"\xCC"

builder.put(CODE_RVA, bytes(code))
size = builder.build("examples/hello/hello.exe")
print(f"wrote examples/hello/hello.exe ({size} bytes)")
