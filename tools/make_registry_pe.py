"""Generate examples/registry/registry.exe: exercise the advapi32 registry APIs."""

import struct

from pebuild import PEBuilder, IMAGE_SUBSYSTEM_WINDOWS_CUI, SECTION_RVA, call_iat, lea_rip

CODE_RVA = 0x1000
SUBKEY_RVA = 0x1200
VALUE_NAME_RVA = 0x1280
VALUE_DATA_RVA = 0x12c0
OK_TEXT_RVA = 0x1320
HKEY_SLOT_RVA = 0x1400
DISP_SLOT_RVA = 0x1410
TYPE_SLOT_RVA = 0x1420
SIZE_SLOT_RVA = 0x1430
BUFFER_RVA = 0x1480

subkey = "Software\\N2NP\\RegistryTest"
value_name = "Greeting"
value_data = "hello from registry"

imports = {
    "advapi32.dll": [
        "RegCreateKeyExW",
        "RegSetValueExW",
        "RegQueryValueExW",
        "RegCloseKey",
    ],
    "msvcrt.dll": ["puts"],
    "kernel32.dll": ["ExitProcess"],
}

builder = PEBuilder(IMAGE_SUBSYSTEM_WINDOWS_CUI, CODE_RVA, section_size=0x3000)
iat = builder.add_imports(imports)

builder.put(SUBKEY_RVA, (subkey + "\0").encode("utf-16le"))
builder.put(VALUE_NAME_RVA, (value_name + "\0").encode("utf-16le"))
builder.put(VALUE_DATA_RVA, (value_data + "\0").encode("utf-16le"))
builder.put(OK_TEXT_RVA, b"registry ok\n\0")
builder.put(HKEY_SLOT_RVA, b"\0" * 8)
builder.put(DISP_SLOT_RVA, b"\0" * 4)
builder.put(TYPE_SLOT_RVA, b"\0" * 4)
builder.put(SIZE_SLOT_RVA, struct.pack("<I", 128))
builder.put(BUFFER_RVA, b"\0" * 128)

code = bytearray()
jumps = []


def mov_ecx_imm(value):
    code.extend(b"\xB9" + struct.pack("<I", value & 0xFFFFFFFF))


def mov_stack_qword(offset, value):
    code.extend(b"\x48\xC7\x44\x24" + bytes([offset]) + struct.pack("<i", value))


def lea_rax(target):
    lea_rip(code, SECTION_RVA, 0x48, 0x05, target)


def lea_rdx(target):
    lea_rip(code, SECTION_RVA, 0x48, 0x15, target)


def lea_r9(target):
    lea_rip(code, SECTION_RVA, 0x4C, 0x0D, target)


def jne_fail():
    code.extend(b"\x85\xC0\x0F\x85\x00\x00\x00\x00")
    jumps.append(len(code) - 4)


code += b"\x48\x83\xEC\x48"
mov_ecx_imm(0x80000001)
lea_rdx(SUBKEY_RVA)
code += b"\x45\x31\xC0"
code += b"\x45\x31\xC9"
mov_stack_qword(0x20, 0)
mov_stack_qword(0x28, 0xF003F)
mov_stack_qword(0x30, 0)
lea_rax(HKEY_SLOT_RVA)
code += b"\x48\x89\x44\x24\x38"
lea_rax(DISP_SLOT_RVA)
code += b"\x48\x89\x44\x24\x40"
call_iat(code, SECTION_RVA, iat["RegCreateKeyExW"])
jne_fail()

code += b"\x48\x8B\x1D"
code += struct.pack("<i", HKEY_SLOT_RVA - (SECTION_RVA + len(code) + 4))
code += b"\x48\x89\xD9"
lea_rdx(VALUE_NAME_RVA)
code += b"\x45\x31\xC0"
code += b"\x41\xB9" + struct.pack("<I", 1)
lea_rax(VALUE_DATA_RVA)
code += b"\x48\x89\x44\x24\x20"
mov_stack_qword(0x28, (len(value_data) + 1) * 2)
call_iat(code, SECTION_RVA, iat["RegSetValueExW"])
jne_fail()

code += b"\x48\x89\xD9"
lea_rdx(VALUE_NAME_RVA)
code += b"\x45\x31\xC0"
lea_r9(TYPE_SLOT_RVA)
lea_rax(BUFFER_RVA)
code += b"\x48\x89\x44\x24\x20"
lea_rax(SIZE_SLOT_RVA)
code += b"\x48\x89\x44\x24\x28"
call_iat(code, SECTION_RVA, iat["RegQueryValueExW"])
jne_fail()

code += b"\x48\x89\xD9"
call_iat(code, SECTION_RVA, iat["RegCloseKey"])

lea_rax(OK_TEXT_RVA)
code += b"\x48\x89\xC1"
call_iat(code, SECTION_RVA, iat["puts"])
code += b"\x31\xC9"
call_iat(code, SECTION_RVA, iat["ExitProcess"])
code += b"\xCC"

fail_rva = SECTION_RVA + len(code)
mov_ecx_imm(1)
call_iat(code, SECTION_RVA, iat["ExitProcess"])
code += b"\xCC"
for position in jumps:
    struct.pack_into("<i", code, position, fail_rva - (SECTION_RVA + position + 4))

assert len(code) < SUBKEY_RVA - CODE_RVA, len(code)
builder.put(CODE_RVA, bytes(code))
size = builder.build("examples/registry/registry.exe")
print(f"wrote examples/registry/registry.exe ({size} bytes)")
