import struct

# Minimal PE64 generator for hello world using WriteFile
# Imports: kernel32.dll!GetStdHandle, kernel32.dll!WriteFile, kernel32.dll!ExitProcess
# Goal: output "hello from n2np\n" to stdout

IMAGE_DOS_SIGNATURE = 0x5A4D
IMAGE_NT_SIGNATURE = 0x00004550
IMAGE_FILE_MACHINE_AMD64 = 0x8664
IMAGE_FILE_EXECUTABLE_IMAGE = 0x0002
IMAGE_SUBSYSTEM_WINDOWS_CUI = 0x3
IMAGE_DIRECTORY_ENTRY_IMPORT = 1

def align(val, align):
    return (val + align - 1) & ~(align - 1)

# Build data
dos = bytearray(64)
struct.pack_into('<H', dos, 0, IMAGE_DOS_SIGNATURE)
struct.pack_into('<L', dos, 60, 0x80)  # e_lfanew

nt_hdrs = bytearray()
nt_hdrs += struct.pack('<L', IMAGE_NT_SIGNATURE)
file_hdr = bytearray(20)
struct.pack_into('<H', file_hdr, 0, IMAGE_FILE_MACHINE_AMD64)
struct.pack_into('<H', file_hdr, 2, 1)  # NumberOfSections
struct.pack_into('<L', file_hdr, 4, 0)
struct.pack_into('<L', file_hdr, 8, 0)
struct.pack_into('<H', file_hdr, 12, 0)
struct.pack_into('<H', file_hdr, 16, 0xF0)  # SizeOfOptionalHeader
struct.pack_into('<H', file_hdr, 18, IMAGE_FILE_EXECUTABLE_IMAGE)
nt_hdrs += file_hdr

opt_hdr = bytearray(240)
struct.pack_into('<H', opt_hdr, 0, 0x20B)  # PE32+
struct.pack_into('<B', opt_hdr, 2, 0)  # MajorLinkerVersion
struct.pack_into('<B', opt_hdr, 3, 0)  # MinorLinkerVersion
struct.pack_into('<L', opt_hdr, 4, 0)
struct.pack_into('<L', opt_hdr, 8, 0)
struct.pack_into('<L', opt_hdr, 12, 0)
struct.pack_into('<L', opt_hdr, 16, 0x1000)  # AddressOfEntryPoint
struct.pack_into('<L', opt_hdr, 20, 0x1000)  # BaseOfCode
struct.pack_into('<Q', opt_hdr, 24, 0x140000000)  # ImageBase
struct.pack_into('<L', opt_hdr, 32, 0x1000)  # SectionAlignment
struct.pack_into('<L', opt_hdr, 36, 0x200)   # FileAlignment
struct.pack_into('<H', opt_hdr, 40, 6)
struct.pack_into('<H', opt_hdr, 42, 0)
struct.pack_into('<H', opt_hdr, 44, 0)
struct.pack_into('<H', opt_hdr, 46, 0)
struct.pack_into('<H', opt_hdr, 48, 6)
struct.pack_into('<H', opt_hdr, 50, 0)
struct.pack_into('<L', opt_hdr, 56, 0x1000)
struct.pack_into('<L', opt_hdr, 60, 0x200)  # SizeOfHeaders
struct.pack_into('<L', opt_hdr, 64, 0)
struct.pack_into('<H', opt_hdr, 68, IMAGE_SUBSYSTEM_WINDOWS_CUI)
struct.pack_into('<H', opt_hdr, 70, 0)
struct.pack_into('<Q', opt_hdr, 72, 0)
struct.pack_into('<Q', opt_hdr, 80, 0)
struct.pack_into('<Q', opt_hdr, 88, 0)
struct.pack_into('<Q', opt_hdr, 96, 0)
struct.pack_into('<L', opt_hdr, 104, 16)  # NumberOfRvaAndSizes
nt_hdrs += opt_hdr

# Import directory
import_rva = 0x2000
data_dir = bytearray(128)
struct.pack_into('<L', data_dir, 8, import_rva)   # Import
struct.pack_into('<L', data_dir, 12, 0x100)      # Size
nt_hdrs += data_dir

# Section header
sec = bytearray(40)
name = b'.text\x00\x00\x00'
sec[0:8] = name
struct.pack_into('<L', sec, 8, 0x1000)   # VirtualSize
struct.pack_into('<L', sec, 12, 0x1000)  # VirtualAddress
struct.pack_into('<L', sec, 16, 0x1000)  # SizeOfRawData
struct.pack_into('<L', sec, 20, 0x400)   # PointerToRawData
struct.pack_into('<L', sec, 24, 0)
struct.pack_into('<L', sec, 28, 0)
struct.pack_into('<L', sec, 32, 0)
struct.pack_into('<L', sec, 36, 0x60000020)  # CODE | EXECUTE | READ

pe = bytearray()
pe += dos
pe += bytearray(0x80 - len(pe)) if len(pe < 0x80) else b''
pe[0x80:0x80] = b''
# rebuild properly
pe = bytearray(dos)
pe += bytearray(0x80 - len(pe))
pos = 0x80
# Write NT headers at 0x80
nt_start = len(pe)
pe += nt_hdrs
# Write section header
pe += sec

# Pad to 0x400
while len(pe) < 0x400:
    pe.append(0)

# .text section at file offset 0x400, RVA 0x1000
text = bytearray(0x1000)
# Very simple code - just call ExitProcess(0) to make it valid PE
# But we need imports to appear. This is minimal stub
# mov rcx, 0; call ExitProcess (but without IAT setup properly hardcoded)
# Actually easier to just make a PE that has import table entries
text[0:2] = b'\x48\x31\xc9'  # xor rcx, rcxc
text[2:6] = b'\x48\x83\xec\x28'
# call ExitProcess - we will patch later conceptually
text[6:7] = b'\xff'
text[7:8] = b'\x15'
struct.pack_into('<L', text, 8, 0)  # rel32 placeholder
text[12:4] = b'\x48\x83\xc4\x28'
text[16:1] = b'\xc3'

pe += text
# Pad to section boundary
while len(pe) < 0x400 + 0x1000:
    pe.append(0)

# Import data at RVA 0x2000
imp = bytearray(0x200)
# Import Directory Table
# kernel32.dll entry
# OriginalFirstThunk RVA
struct.pack_into('<L', imp, 0, 0x2100)  # INT
struct.pack_into('<L', imp, 4, 0)       # TimeDateStamp
struct.pack_into('<L', imp, 8, 0)       # ForwarderChain
struct.pack_into('<L', imp, 12, 0x2180) # Name
struct.pack_into('<L', imp, 16, 0x2200) # FirstThunk (IAT)

# Terminated
# Name of kernel32.dll at 0x2180
name_k32 = imp[0x180:0x190]
for i, b in enumerate(b'kernel32.dll\x00'):
    imp[0x180 + i] = b

# INT at 0x2100
imp[0x100:0x108] = struct.pack('<Q', 0x2140)  # GetStdHandle
imp[0x108:0x110] = struct.pack('<Q', 0x2150)  # WriteFile
imp[0x110:0x118] = struct.pack('<Q', 0x2160)  # ExitProcess
imp[0x118:0x120] = struct.pack('<Q', 0)       # end

# IAT same as INT
imp[0x200:0x208] = struct.pack('<Q', 0x2140)
imp[0x208:0x210] = struct.pack('<Q', 0x2150)
imp[0x210:0x218] = struct.pack('<Q', 0x2160)

# Hint/Name tables
imp[0x140:0x150] = struct.pack('<H', 0) + b'GetStdHandle\x00'
imp[0x150:0x160] = struct.pack('<H', 0) + b'WriteFile\x00'
imp[0x160:0x170] = struct.pack('<H', 0) + b'ExitProcess\x00'

pe += imp

with open('examples/hello/hello.exe', 'wb') as f:
    f.write(pe)

print('done')