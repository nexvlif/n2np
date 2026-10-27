import struct

IMAGE_DOS_SIGNATURE = 0x5A4D
IMAGE_NT_SIGNATURE = 0x00004550
IMAGE_FILE_MACHINE_AMD64 = 0x8664
IMAGE_FILE_EXECUTABLE_IMAGE = 0x0002
IMAGE_FILE_LARGE_ADDRESS_AWARE = 0x0020
IMAGE_SUBSYSTEM_WINDOWS_GUI = 0x2
IMAGE_SCN_CNT_CODE = 0x00000020
IMAGE_SCN_MEM_EXECUTE = 0x20000000
IMAGE_SCN_MEM_READ = 0x40000000
IMAGE_SCN_MEM_WRITE = 0x80000000

FILE_ALIGNMENT = 0x200
SECTION_ALIGNMENT = 0x1000
SECTION_RVA = 0x1000
SECTION_RAW = 0x200
SECTION_SIZE = 0x1000

TEXT_RVA = 0x1100
CAPTION_RVA = 0x1180
IMPORT_RVA = 0x1200
USER32_INT_RVA = 0x1300
KERNEL32_INT_RVA = 0x1310
USER32_IAT_RVA = 0x1400
KERNEL32_IAT_RVA = 0x1408
MESSAGEBOX_NAME_RVA = 0x1500
EXITPROCESS_NAME_RVA = 0x1520
USER32_DLL_RVA = 0x1580
KERNEL32_DLL_RVA = 0x1590


def align(value, alignment):
    return (value + alignment - 1) & ~(alignment - 1)


def rva_to_section_offset(rva):
    assert SECTION_RVA <= rva < SECTION_RVA + SECTION_SIZE, hex(rva)
    return rva - SECTION_RVA


def lea_rip(reg_rex, reg_modrm, next_rva, target_rva):
    disp = target_rva - next_rva
    assert -0x80000000 <= disp <= 0x7FFFFFFF, hex(disp)
    return bytes([reg_rex, 0x8D, reg_modrm]) + struct.pack("<i", disp)


def call_iat(next_rva, iat_rva):
    disp = iat_rva - next_rva
    assert -0x80000000 <= disp <= 0x7FFFFFFF, hex(disp)
    return b"\xFF\x15" + struct.pack("<i", disp)


text = (
    "Hello from n2np GUI!\n\n"
    "This MessageBoxA is running from a Windows PE binary through n2np on Linux/WSLg.\n\n"
    "Click OK to continue.\0"
).encode("utf-8")
caption = b"n2np / WSLg\0"
assert len(text) <= CAPTION_RVA - TEXT_RVA, len(text)

code = bytearray()
code += b"\x48\x83\xEC\x28"  # sub rsp, 0x28
code += b"\x31\xC9"  # xor ecx, ecx
code += lea_rip(0x48, 0x15, SECTION_RVA + 0x0D, TEXT_RVA)  # lea rdx, [rip+text]
code += lea_rip(0x4C, 0x05, SECTION_RVA + 0x14, CAPTION_RVA)  # lea r8, [rip+caption]
code += b"\x45\x31\xC9"  # xor r9d, r9d
code += call_iat(SECTION_RVA + 0x1D, USER32_IAT_RVA)  # MessageBoxA
code += b"\x31\xC9"  # xor ecx, ecx
code += call_iat(SECTION_RVA + 0x25, KERNEL32_IAT_RVA)  # ExitProcess
code += b"\xCC"  # int3
assert len(code) == 0x26, len(code)

section = bytearray(SECTION_SIZE)

def put(rva, data):
    off = rva_to_section_offset(rva)
    end = off + len(data)
    assert end <= len(section), hex(rva)
    section[off:end] = data


put(SECTION_RVA, code)
put(TEXT_RVA, text)
put(CAPTION_RVA, caption)

descriptor = struct.pack(
    "<IIIII",
    USER32_INT_RVA,
    0,
    0,
    USER32_DLL_RVA,
    USER32_IAT_RVA,
)
descriptor += struct.pack(
    "<IIIII",
    KERNEL32_INT_RVA,
    0,
    0,
    KERNEL32_DLL_RVA,
    KERNEL32_IAT_RVA,
)
descriptor += b"\0" * 20
put(IMPORT_RVA, descriptor)

put(USER32_INT_RVA, struct.pack("<Q", MESSAGEBOX_NAME_RVA) + b"\0" * 8)
put(KERNEL32_INT_RVA, struct.pack("<Q", EXITPROCESS_NAME_RVA) + b"\0" * 8)
put(USER32_IAT_RVA, struct.pack("<Q", MESSAGEBOX_NAME_RVA))
put(KERNEL32_IAT_RVA, struct.pack("<Q", EXITPROCESS_NAME_RVA))
put(MESSAGEBOX_NAME_RVA, struct.pack("<H", 0) + b"MessageBoxA\0")
put(EXITPROCESS_NAME_RVA, struct.pack("<H", 0) + b"ExitProcess\0")
put(USER32_DLL_RVA, b"user32.dll\0")
put(KERNEL32_DLL_RVA, b"kernel32.dll\0")

dos = bytearray(0x40)
struct.pack_into("<H", dos, 0, IMAGE_DOS_SIGNATURE)
struct.pack_into("<I", dos, 0x3C, 0x80)

file_header = struct.pack(
    "<HHIIIHH",
    IMAGE_FILE_MACHINE_AMD64,
    1,
    0,
    0,
    0,
    0xF0,
    IMAGE_FILE_EXECUTABLE_IMAGE | IMAGE_FILE_LARGE_ADDRESS_AWARE,
)

optional = bytearray(0xF0)
struct.pack_into("<H", optional, 0, 0x20B)
struct.pack_into("<B", optional, 2, 1)
struct.pack_into("<B", optional, 3, 0)
struct.pack_into("<I", optional, 4, SECTION_SIZE)
struct.pack_into("<I", optional, 8, SECTION_SIZE)
struct.pack_into("<I", optional, 12, 0)
struct.pack_into("<I", optional, 16, SECTION_RVA)
struct.pack_into("<I", optional, 20, SECTION_RVA)
struct.pack_into("<Q", optional, 24, 0x140000000)
struct.pack_into("<I", optional, 32, SECTION_ALIGNMENT)
struct.pack_into("<I", optional, 36, FILE_ALIGNMENT)
struct.pack_into("<H", optional, 40, 6)
struct.pack_into("<H", optional, 42, 0)
struct.pack_into("<H", optional, 44, 0)
struct.pack_into("<H", optional, 46, 0)
struct.pack_into("<H", optional, 48, 6)
struct.pack_into("<H", optional, 50, 0)
struct.pack_into("<I", optional, 56, 0x2000)
struct.pack_into("<I", optional, 60, FILE_ALIGNMENT)
struct.pack_into("<I", optional, 64, 0)
struct.pack_into("<H", optional, 68, IMAGE_SUBSYSTEM_WINDOWS_GUI)
struct.pack_into("<H", optional, 70, 0)
struct.pack_into("<Q", optional, 72, 0x100000)
struct.pack_into("<Q", optional, 80, 0x1000)
struct.pack_into("<Q", optional, 88, 0x100000)
struct.pack_into("<Q", optional, 96, 0x1000)
struct.pack_into("<I", optional, 104, 0)
struct.pack_into("<I", optional, 108, 16)

data_dirs = 112
struct.pack_into("<II", optional, data_dirs + 8, IMPORT_RVA, len(descriptor))
struct.pack_into("<II", optional, data_dirs + 96, USER32_IAT_RVA, 16)

section_header = bytearray(40)
section_header[0:8] = b".text\0\0\0"
struct.pack_into("<I", section_header, 8, SECTION_SIZE)
struct.pack_into("<I", section_header, 12, SECTION_RVA)
struct.pack_into("<I", section_header, 16, SECTION_SIZE)
struct.pack_into("<I", section_header, 20, SECTION_RAW)
struct.pack_into("<I", section_header, 24, 0)
struct.pack_into("<I", section_header, 28, 0)
struct.pack_into("<H", section_header, 32, 0)
struct.pack_into("<H", section_header, 34, 0)
struct.pack_into(
    "<I",
    section_header,
    36,
    IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE,
)

headers = bytearray(dos)
headers += b"\0" * (0x80 - len(headers))
headers += struct.pack("<I", IMAGE_NT_SIGNATURE)
headers += file_header
headers += optional
headers += section_header
headers += b"\0" * (FILE_ALIGNMENT - len(headers))
assert len(headers) == FILE_ALIGNMENT, len(headers)

output = bytes(headers) + bytes(section)
with open("examples/gui/gui.exe", "wb") as f:
    f.write(output)

print(f"wrote examples/gui/gui.exe ({len(output)} bytes)")
