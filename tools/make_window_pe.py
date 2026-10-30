"""Generate examples/window/window.exe: a real Win32 window with a message loop.

The executable registers a window class, creates a top-level window, paints two
lines of text with GDI, and runs the classic GetMessage/TranslateMessage/
DispatchMessage loop. It imports only the APIs n2np implements for this demo.
"""

import struct

from pebuild import (
    PEBuilder,
    IMAGE_SUBSYSTEM_WINDOWS_GUI,
    SECTION_RVA,
    call_iat,
    lea_rip,
)

ENTRY_RVA = 0x1000
WNDPROC_RVA = 0x1200
DATA_RVA = 0x1400
WNDCLASS_RVA = DATA_RVA
CLASS_NAME_RVA = 0x1450
TITLE_RVA = 0x1470
TEXT1_RVA = 0x1490
TEXT2_RVA = 0x14C0
MSG_RVA = 0x1500
PAINT_RVA = 0x1540
HWND_RVA = 0x1590

IMAGE_BASE = 0x140000000

text1 = b"Hello from a real Win32 window!\0"
text2 = b"n2np created this window through Wayland.\0"
class_name = b"n2npWindowClass\0"
title = b"n2np native window\0"

assert len(text1) <= TEXT2_RVA - TEXT1_RVA
assert len(text2) <= MSG_RVA - TEXT2_RVA

imports = {
    "kernel32.dll": ["ExitProcess"],
    "user32.dll": [
        "RegisterClassA",
        "CreateWindowExA",
        "ShowWindow",
        "UpdateWindow",
        "GetMessageA",
        "TranslateMessage",
        "DispatchMessageA",
        "DefWindowProcA",
        "BeginPaint",
        "EndPaint",
        "DestroyWindow",
        "PostQuitMessage",
        "SetTimer",
        "InvalidateRect",
    ],
    "gdi32.dll": ["TextOutA"],
}

builder = PEBuilder(IMAGE_SUBSYSTEM_WINDOWS_GUI, ENTRY_RVA, section_size=0x3000)
iat = builder.add_imports(imports)


class Asm:
    def __init__(self, base_rva):
        self.base = base_rva
        self.buf = bytearray()
        self.labels = {}
        self.fixups = []

    def rva(self):
        return self.base + len(self.buf)

    def label(self, name):
        self.labels[name] = self.rva()

    def bytes(self, data):
        self.buf += data

    def rel(self, opcode, label):
        self.buf += opcode
        offset = len(self.buf)
        self.fixups.append((offset, label))
        self.buf += b"\0\0\0\0"

    def jmp(self, label):
        self.rel(b"\xE9", label)

    def jz(self, label):
        self.rel(b"\x0F\x84", label)

    def jle(self, label):
        self.rel(b"\x0F\x8E", label)

    def call_iat(self, name):
        call_iat(self.buf, self.base, iat[name])

    def lea(self, reg, target_rva):
        # reg is one of rcx, rdx, r8, r9, rax
        table = {
            "rax": (0x48, 0x05),
            "rcx": (0x48, 0x0D),
            "rdx": (0x48, 0x15),
            "r8": (0x4C, 0x05),
            "r9": (0x4C, 0x0D),
        }
        rex, modrm = table[reg]
        lea_rip(self.buf, self.base, rex, modrm, target_rva)

    def load_rax_rip(self, target_rva):
        self.buf += b"\x48\x8B\x05"
        offset = len(self.buf)
        self.buf += b"\0\0\0\0"
        self.fixups.append((offset, ("abs", target_rva)))

    def load_rcx_rip(self, target_rva):
        self.buf += b"\x48\x8B\x0D"
        offset = len(self.buf)
        self.buf += b"\0\0\0\0"
        self.fixups.append((offset, ("abs", target_rva)))

    def store_rax_rip(self, target_rva):
        self.buf += b"\x48\x89\x05"
        offset = len(self.buf)
        self.buf += b"\0\0\0\0"
        self.fixups.append((offset, ("abs", target_rva)))

    def mov_rsp_imm32(self, disp, value):
        if value > 0x7FFFFFFF:
            value -= 0x1_0000_0000
        self.buf += b"\x48\xC7\x44\x24" + bytes([disp]) + struct.pack("<i", value)

    def resolve(self):
        for offset, label in self.fixups:
            if isinstance(label, tuple) and label[0] == "abs":
                target = label[1]
            else:
                target = self.labels[label]
            disp = target - (self.base + offset + 4)
            struct.pack_into("<i", self.buf, offset, disp)
        return bytes(self.buf)


# ---------------------------------------------------------------------------
# Entry point / message loop
# ---------------------------------------------------------------------------
main = Asm(ENTRY_RVA)
main.bytes(b"\x48\x83\xEC\x68")  # sub rsp, 0x68
main.lea("rcx", WNDCLASS_RVA)
main.call_iat("RegisterClassA")
main.bytes(b"\x85\xC0")  # test eax, eax
main.jz("fail")

main.bytes(b"\x31\xC9")  # xor ecx, ecx
main.lea("rdx", CLASS_NAME_RVA)
main.lea("r8", TITLE_RVA)
main.bytes(b"\x41\xB9" + struct.pack("<I", 0x00CF0000))  # WS_OVERLAPPEDWINDOW
main.mov_rsp_imm32(0x20, 0x80000000)  # CW_USEDEFAULT x
main.mov_rsp_imm32(0x28, 0x80000000)  # CW_USEDEFAULT y
main.mov_rsp_imm32(0x30, 720)
main.mov_rsp_imm32(0x38, 420)
main.mov_rsp_imm32(0x40, 0)  # parent
main.mov_rsp_imm32(0x48, 0)  # menu
main.mov_rsp_imm32(0x50, 0)  # instance
main.mov_rsp_imm32(0x58, 0)  # create param
main.call_iat("CreateWindowExA")
main.bytes(b"\x48\x85\xC0")  # test rax, rax
main.jz("fail")
main.store_rax_rip(HWND_RVA)

main.load_rcx_rip(HWND_RVA)
main.bytes(b"\xBA" + struct.pack("<I", 5))  # mov edx, SW_SHOW
main.call_iat("ShowWindow")
main.load_rcx_rip(HWND_RVA)
main.call_iat("UpdateWindow")

# SetTimer(hwnd, 1, 100, NULL) - repaint every 100 ms through WM_TIMER.
main.load_rcx_rip(HWND_RVA)
main.bytes(b"\xBA" + struct.pack("<I", 1))  # timer id
main.bytes(b"\x41\xB8" + struct.pack("<I", 100))  # elapse
main.bytes(b"\x45\x31\xC9")  # xor r9d, r9d (no callback)
main.call_iat("SetTimer")

main.label("loop")
main.lea("rcx", MSG_RVA)
main.bytes(b"\x31\xD2")  # xor edx, edx
main.bytes(b"\x45\x31\xC0")  # xor r8d, r8d
main.bytes(b"\x45\x31\xC9")  # xor r9d, r9d
main.call_iat("GetMessageA")
main.bytes(b"\x85\xC0")  # test eax, eax
main.jle("done")
main.lea("rcx", MSG_RVA)
main.call_iat("TranslateMessage")
main.lea("rcx", MSG_RVA)
main.call_iat("DispatchMessageA")
main.jmp("loop")

main.label("done")
main.bytes(b"\x31\xC9")  # xor ecx, ecx
main.call_iat("ExitProcess")

main.label("fail")
main.bytes(b"\xB9" + struct.pack("<I", 1))  # mov ecx, 1
main.call_iat("ExitProcess")

# ---------------------------------------------------------------------------
# Window procedure
# ---------------------------------------------------------------------------
wndproc = Asm(WNDPROC_RVA)
wndproc.bytes(b"\x53")  # push rbx
wndproc.bytes(b"\x48\x83\xEC\x30")  # sub rsp, 0x30
wndproc.bytes(b"\x48\x89\xCB")  # mov rbx, rcx

wndproc.bytes(b"\x81\xFA" + struct.pack("<I", 0x0002))  # cmp edx, WM_DESTROY
wndproc.jz("wm_destroy")
wndproc.bytes(b"\x81\xFA" + struct.pack("<I", 0x000F))  # cmp edx, WM_PAINT
wndproc.jz("wm_paint")
wndproc.bytes(b"\x81\xFA" + struct.pack("<I", 0x0010))  # cmp edx, WM_CLOSE
wndproc.jz("wm_close")
wndproc.bytes(b"\x81\xFA" + struct.pack("<I", 0x0113))  # cmp edx, WM_TIMER
wndproc.jz("wm_timer")

# DefWindowProcA(hwnd, msg, wparam, lparam)
wndproc.bytes(b"\x48\x89\xD9")  # mov rcx, rbx
wndproc.call_iat("DefWindowProcA")
wndproc.jmp("wnd_done")

wndproc.label("wm_destroy")
wndproc.bytes(b"\x31\xC9")  # xor ecx, ecx
wndproc.call_iat("PostQuitMessage")
wndproc.bytes(b"\x31\xC0")  # xor eax, eax
wndproc.jmp("wnd_done")

wndproc.label("wm_paint")
wndproc.bytes(b"\x48\x89\xD9")  # mov rcx, rbx
wndproc.lea("rdx", PAINT_RVA)
wndproc.call_iat("BeginPaint")
wndproc.bytes(b"\x48\x89\xC1")  # mov rcx, rax
wndproc.bytes(b"\xBA" + struct.pack("<I", 24))  # mov edx, 24
wndproc.bytes(b"\x41\xB8" + struct.pack("<I", 48))  # mov r8d, 48
wndproc.lea("r9", TEXT1_RVA)
wndproc.mov_rsp_imm32(0x20, len(text1) - 1)
wndproc.call_iat("TextOutA")

wndproc.load_rcx_rip(PAINT_RVA)  # hdc = paint.hdc
wndproc.bytes(b"\xBA" + struct.pack("<I", 24))
wndproc.bytes(b"\x41\xB8" + struct.pack("<I", 76))
wndproc.lea("r9", TEXT2_RVA)
wndproc.mov_rsp_imm32(0x20, len(text2) - 1)
wndproc.call_iat("TextOutA")

wndproc.bytes(b"\x48\x89\xD9")  # mov rcx, rbx
wndproc.lea("rdx", PAINT_RVA)
wndproc.call_iat("EndPaint")
wndproc.bytes(b"\x31\xC0")
wndproc.jmp("wnd_done")

wndproc.label("wm_timer")
wndproc.bytes(b"\x48\x89\xD9")  # mov rcx, rbx
wndproc.bytes(b"\x31\xD2")  # xor edx, edx (rect = NULL)
wndproc.bytes(b"\x41\xB8" + struct.pack("<I", 1))  # erase = TRUE
wndproc.call_iat("InvalidateRect")
wndproc.bytes(b"\x31\xC0")
wndproc.jmp("wnd_done")

wndproc.label("wm_close")
wndproc.bytes(b"\x48\x89\xD9")  # mov rcx, rbx
wndproc.call_iat("DestroyWindow")
wndproc.bytes(b"\x31\xC0")
wndproc.jmp("wnd_done")

wndproc.label("wnd_done")
wndproc.bytes(b"\x48\x83\xC4\x30")  # add rsp, 0x30
wndproc.bytes(b"\x5B")  # pop rbx
wndproc.bytes(b"\xC3")  # ret

main_code = main.resolve()
wndproc_code = wndproc.resolve()
assert len(main_code) <= WNDPROC_RVA - ENTRY_RVA
assert len(wndproc_code) <= DATA_RVA - WNDPROC_RVA

builder.put(ENTRY_RVA, main_code)
builder.put(WNDPROC_RVA, wndproc_code)

# WNDCLASSA: style, lpfnWndProc, cbClsExtra, cbWndExtra, hInstance,
# hIcon, hCursor, hbrBackground, lpszMenuName, lpszClassName.
wndclass = bytearray(72)
struct.pack_into("<I", wndclass, 0, 0x0003)  # CS_HREDRAW | CS_VREDRAW
struct.pack_into("<Q", wndclass, 8, IMAGE_BASE + WNDPROC_RVA)
struct.pack_into("<Q", wndclass, 64, IMAGE_BASE + CLASS_NAME_RVA)
builder.put(WNDCLASS_RVA, bytes(wndclass))
builder.put(CLASS_NAME_RVA, class_name)
builder.put(TITLE_RVA, title)
builder.put(TEXT1_RVA, text1)
builder.put(TEXT2_RVA, text2)

size = builder.build("examples/window/window.exe")
print(f"wrote examples/window/window.exe ({size} bytes)")
