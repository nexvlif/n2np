"""Generate examples/gui/gui.exe: a GUI PE that shows a MessageBoxA via user32."""

import struct

from pebuild import PEBuilder, IMAGE_SUBSYSTEM_WINDOWS_GUI, SECTION_RVA, call_iat, lea_rip

CODE_RVA = 0x1000
TEXT_RVA = 0x1100
CAPTION_RVA = 0x1200

message = (
    "Hello from n2np GUI!\n\n"
    "This MessageBoxA is running from a Windows PE binary through n2np on Linux/Wayland.\n\n"
    "Click OK to continue.\0"
).encode("utf-8")
caption = b"n2np / Wayland\0"
assert len(message) < CAPTION_RVA - TEXT_RVA

imports = {
    "user32.dll": ["MessageBoxA"],
    "kernel32.dll": ["ExitProcess"],
}

builder = PEBuilder(IMAGE_SUBSYSTEM_WINDOWS_GUI, CODE_RVA, section_size=0x3000)
iat = builder.add_imports(imports)

builder.put(TEXT_RVA, message)
builder.put(CAPTION_RVA, caption)

code = bytearray()
code += b"\x48\x83\xEC\x28"  # sub rsp, 0x28
code += b"\x31\xC9"  # xor ecx, ecx
lea_rip(code, SECTION_RVA, 0x48, 0x15, TEXT_RVA)  # lea rdx, [rip+message]
lea_rip(code, SECTION_RVA, 0x4C, 0x05, CAPTION_RVA)  # lea r8, [rip+caption]
code += b"\x45\x31\xC9"  # xor r9d, r9d
call_iat(code, SECTION_RVA, iat["MessageBoxA"])
code += b"\x31\xC9"  # xor ecx, ecx
call_iat(code, SECTION_RVA, iat["ExitProcess"])
code += b"\xCC"

builder.put(CODE_RVA, bytes(code))
size = builder.build("examples/gui/gui.exe")
print(f"wrote examples/gui/gui.exe ({size} bytes)")
