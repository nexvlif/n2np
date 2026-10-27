import struct

IMAGE_DOS_SIGNATURE = 0x5A4D
IMAGE_NT_SIGNATURE = 0x00004550
IMAGE_FILE_MACHINE_AMD64 = 0x8664
IMAGE_FILE_EXECUTABLE_IMAGE = 0x0002
IMAGE_SUBSYSTEM_WINDOWS_CUI = 0x3

pe = bytearray()
dos = bytearray(64)
struct.pack_into("<H", dos, 0, IMAGE_DOS_SIGNATURE)
struct.pack_into("<L", dos, 60, 0x80)
pe += dos
while len(pe) < 0x80:
    pe.append(0)

nt = bytearray()
nt += struct.pack("<L", IMAGE_NT_SIGNATURE)
fh = bytearray(20)
struct.pack_into("<H", fh, 0, IMAGE_FILE_MACHINE_AMD64)
struct.pack_into("<H", fh, 2, 1)
struct.pack_into("<H", fh, 16, 0xF0)
struct.pack_into("<H", fh, 18, IMAGE_FILE_EXECUTABLE_IMAGE)
nt += fh
oh = bytearray(240)
struct.pack_into("<H", oh, 0, 0x20B)
struct.pack_into("<L", oh, 16, 0x1000)
struct.pack_into("<L", oh, 20, 0x1000)
struct.pack_into("<Q", oh, 24, 0x140000000)
struct.pack_into("<L", oh, 32, 0x1000)
struct.pack_into("<L", oh, 36, 0x200)
struct.pack_into("<L", oh, 60, 0x200)
struct.pack_into("<H", oh, 68, IMAGE_SUBSYSTEM_WINDOWS_CUI)
struct.pack_into("<L", oh, 104, 16)
nt += oh
dd = bytearray(128)
struct.pack_into("<L", dd, 8, 0x2000)
struct.pack_into("<L", dd, 12, 0x100)
nt += dd
pe += nt

sh = bytearray(40)
sh[0:6] = b".text\x00"
struct.pack_into("<L", sh, 8, 0x100)
struct.pack_into("<L", sh, 12, 0x1000)
struct.pack_into("<L", sh, 16, 0x200)
struct.pack_into("<L", sh, 20, 0x200)
struct.pack_into("<L", sh, 36, 0x60000020)
pe += sh

while len(pe) < 0x200:
    pe.append(0)

text = bytearray(0x200)
pe += text

imp = bytearray(0x400)
struct.pack_into("<L", imp, 0, 0x2100)
struct.pack_into("<L", imp, 12, 0x2180)
struct.pack_into("<L", imp, 16, 0x2200)
imp[0x180:0x190] = b"kernel32.dll\x00"
imp[0x100:0x108] = struct.pack("<Q", 0x2140)
imp[0x108:0x110] = struct.pack("<Q", 0x2150)
imp[0x110:0x118] = struct.pack("<Q", 0x2160)
imp[0x200:0x208] = struct.pack("<Q", 0x2140)
imp[0x208:0x210] = struct.pack("<Q", 0x2150)
imp[0x210:0x218] = struct.pack("<Q", 0x2160)
imp[0x140:0x150] = struct.pack("<H", 0) + b"GetStdHandle\x00"
imp[0x150:0x160] = struct.pack("<H", 0) + b"WriteFile\x00"
imp[0x160:0x170] = struct.pack("<H", 0) + b"ExitProcess\x00"
pe += imp

with open("examples/hello/hello.exe", "wb") as f:
    f.write(pe)

print("ok")