"""Small, dependency-free PE32+ image builder used by the n2np example generators."""

import struct

IMAGE_DOS_SIGNATURE = 0x5A4D
IMAGE_NT_SIGNATURE = 0x00004550
IMAGE_FILE_MACHINE_AMD64 = 0x8664
IMAGE_FILE_EXECUTABLE_IMAGE = 0x0002
IMAGE_FILE_LARGE_ADDRESS_AWARE = 0x0020
IMAGE_SUBSYSTEM_WINDOWS_GUI = 0x2
IMAGE_SUBSYSTEM_WINDOWS_CUI = 0x3
IMAGE_SCN_CNT_CODE = 0x00000020
IMAGE_SCN_MEM_EXECUTE = 0x20000000
IMAGE_SCN_MEM_READ = 0x40000000
IMAGE_SCN_MEM_WRITE = 0x80000000

FILE_ALIGNMENT = 0x200
SECTION_ALIGNMENT = 0x1000
SECTION_RVA = 0x1000


def align(value, alignment):
    return (value + alignment - 1) & ~(alignment - 1)


class PEBuilder:
    """Builds a single-section x86_64 PE with an import table.

    Code and read-only data live at the start of the section; the import table,
    INT, IAT and hint/name entries are allocated starting at 0x2000.
    """

    def __init__(self, subsystem, entry_rva, section_size=0x3000, image_base=0x140000000):
        self.subsystem = subsystem
        self.entry_rva = entry_rva
        self.section_size = section_size
        self.image_base = image_base
        self.section = bytearray(section_size)
        self.import_rva = 0
        self.import_size = 0
        self._alloc_rva = 0x2000
        self._iat = {}

    def put(self, rva, data):
        offset = rva - SECTION_RVA
        end = offset + len(data)
        if offset < 0 or end > len(self.section):
            raise ValueError(f"data at 0x{rva:x} does not fit in the section")
        self.section[offset:end] = data

    def _alloc(self, size, alignment=8):
        self._alloc_rva = align(self._alloc_rva, alignment)
        rva = self._alloc_rva
        self._alloc_rva += size
        if self._alloc_rva > SECTION_RVA + self.section_size:
            raise ValueError("import data does not fit in the section")
        return rva

    def add_imports(self, imports):
        """Register imports and return {name: iat_rva} for the first DLL.

        imports is an ordered dict: {dll_name: [function_name, ...]}.
        """
        if not imports:
            return {}
        self.import_rva = self._alloc(20 * (len(imports) + 1), 4)
        descriptors = []
        for dll, names in imports.items():
            int_rva = self._alloc(8 * (len(names) + 1))
            iat_rva = self._alloc(8 * (len(names) + 1))
            dll_rva = self._alloc(len(dll) + 1, 1)
            self.put(dll_rva, dll.encode("ascii") + b"\0")
            for index, name in enumerate(names):
                hint_rva = self._alloc(2 + len(name) + 1, 2)
                self.put(hint_rva, struct.pack("<H", 0) + name.encode("ascii") + b"\0")
                struct.pack_into("<Q", self.section, int_rva - SECTION_RVA + 8 * index, hint_rva)
                struct.pack_into("<Q", self.section, iat_rva - SECTION_RVA + 8 * index, hint_rva)
                self._iat[name] = iat_rva + 8 * index
            struct.pack_into("<Q", self.section, int_rva - SECTION_RVA + 8 * len(names), 0)
            struct.pack_into("<Q", self.section, iat_rva - SECTION_RVA + 8 * len(names), 0)
            descriptors.append(struct.pack("<IIIII", int_rva, 0, 0, dll_rva, iat_rva))
        blob = b"".join(descriptors) + b"\0" * 20
        self.put(self.import_rva, blob)
        self.import_size = len(blob)
        return dict(self._iat)

    def build(self, path):
        raw_size = align(self.section_size, FILE_ALIGNMENT)
        size_of_image = align(SECTION_RVA + self.section_size, SECTION_ALIGNMENT)

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
        struct.pack_into("<I", optional, 4, raw_size)
        struct.pack_into("<I", optional, 8, raw_size)
        struct.pack_into("<I", optional, 12, 0)
        struct.pack_into("<I", optional, 16, self.entry_rva)
        struct.pack_into("<I", optional, 20, SECTION_RVA)
        struct.pack_into("<Q", optional, 24, self.image_base)
        struct.pack_into("<I", optional, 32, SECTION_ALIGNMENT)
        struct.pack_into("<I", optional, 36, FILE_ALIGNMENT)
        struct.pack_into("<H", optional, 40, 6)
        struct.pack_into("<H", optional, 42, 0)
        struct.pack_into("<H", optional, 44, 0)
        struct.pack_into("<H", optional, 46, 0)
        struct.pack_into("<H", optional, 48, 6)
        struct.pack_into("<H", optional, 50, 0)
        struct.pack_into("<I", optional, 56, size_of_image)
        struct.pack_into("<I", optional, 60, FILE_ALIGNMENT)
        struct.pack_into("<I", optional, 64, 0)
        struct.pack_into("<H", optional, 68, self.subsystem)
        struct.pack_into("<H", optional, 70, 0)
        struct.pack_into("<Q", optional, 72, 0x100000)
        struct.pack_into("<Q", optional, 80, 0x1000)
        struct.pack_into("<Q", optional, 88, 0x100000)
        struct.pack_into("<Q", optional, 96, 0x1000)
        struct.pack_into("<I", optional, 104, 0)
        struct.pack_into("<I", optional, 108, 16)

        if self.import_rva:
            struct.pack_into("<II", optional, 112 + 8, self.import_rva, self.import_size)

        section_header = bytearray(40)
        section_header[0:8] = b".text\0\0\0"
        struct.pack_into("<I", section_header, 8, self.section_size)
        struct.pack_into("<I", section_header, 12, SECTION_RVA)
        struct.pack_into("<I", section_header, 16, raw_size)
        struct.pack_into("<I", section_header, 20, FILE_ALIGNMENT)
        struct.pack_into("<I", section_header, 24, 0)
        struct.pack_into("<I", section_header, 28, 0)
        struct.pack_into("<H", section_header, 32, 0)
        struct.pack_into("<H", section_header, 34, 0)
        struct.pack_into(
            "<I",
            section_header,
            36,
            IMAGE_SCN_CNT_CODE
            | IMAGE_SCN_MEM_EXECUTE
            | IMAGE_SCN_MEM_READ
            | IMAGE_SCN_MEM_WRITE,
        )

        headers = bytearray(dos)
        headers += b"\0" * (0x80 - len(headers))
        headers += struct.pack("<I", IMAGE_NT_SIGNATURE)
        headers += file_header
        headers += optional
        headers += section_header
        headers += b"\0" * (FILE_ALIGNMENT - len(headers))

        section = bytes(self.section) + b"\0" * (raw_size - len(self.section))
        output = bytes(headers) + section
        with open(path, "wb") as handle:
            handle.write(output)
        return len(output)


def call_iat(code, base_rva, iat_rva):
    next_rva = base_rva + len(code) + 6
    disp = iat_rva - next_rva
    if not -0x80000000 <= disp <= 0x7FFFFFFF:
        raise ValueError("IAT is out of rel32 range")
    code += b"\xFF\x15" + struct.pack("<i", disp)


def lea_rip(code, base_rva, rex, modrm, target_rva):
    next_rva = base_rva + len(code) + 7
    disp = target_rva - next_rva
    if not -0x80000000 <= disp <= 0x7FFFFFFF:
        raise ValueError("target is out of rel32 range")
    code += bytes([rex, 0x8D, modrm]) + struct.pack("<i", disp)
