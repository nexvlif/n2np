# n2np

**n2np** aims to run Windows PE applications on Linux by implementing only the functionality an application actually uses.

The goal is to explore profile-guided Windows API compatibility, using static PE import analysis and runtime tracing to identify required functionality.

## Tools

- **`n2np-trace`** — Generates profiles from PE imports through static analysis and supports collecting runtime usage information.
- **`n2np-runtime`** — Loads an application profile and attempts to execute the target using native implementations of the required functionality.

## Quick Start

Generate a profile from a Windows PE executable:

```bash
cargo run -p n2np-trace -- static \
  --target examples/hello/hello.exe \
  --out profiles/hello-auto.json
```

Run the executable using the generated profile:

```bash
cargo run -p n2np-runtime -- \
  --profile profiles/hello-auto.json \
  --target examples/hello/hello.exe
```

> **Note:** These commands assume the workspace, example executable, and CLI arguments are implemented as shown. Runtime compatibility depends on the APIs and executable features currently supported by n2np.

## Technical References

### Executable Format and Compatibility

- [PE/COFF Specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format) — Windows executable format, headers, sections, imports, and exports.
- [Wine](https://www.winehq.org/) — Windows API compatibility layer for Unix-like systems.
- [FEX-Emu](https://github.com/FEX-Emu/FEX) — User-mode x86 and x86-64 emulation on ARM64 Linux.
- [Box86 / Box64](https://box86.org/) — Compatibility projects for running x86 and x86-64 applications on Linux, particularly on ARM systems.

### Runtime Instrumentation and Debugging

- [Frida](https://frida.re/) — Dynamic instrumentation toolkit for observing and interacting with running programs.
- [x64dbg](https://x64dbg.com/) — Windows user-mode debugger for analyzing executable behavior.
- [MinHook](https://github.com/TsudaKageyu/minhook) — Windows API hooking library for x86 and x64.

### Graphics Compatibility

- [DXVK](https://github.com/doitsujin/dxvk) — Implements Direct3D 8, 9, 10, and 11 over Vulkan.
- [VKD3D-Proton](https://github.com/HansKristian-Work/vkd3d-proton) — Direct3D 12 implementation built on Vulkan for Proton.

## Research

These resources provide starting points for investigating binary translation, runtime instrumentation, and executable compatibility.

- [arXiv: Binary Translation](https://arxiv.org/search/?query=Binary%20Translation&searchtype=all)
- [arXiv: Dynamic Binary Instrumentation](https://arxiv.org/search/?query=Dynamic%20Binary%20Instrumentation&searchtype=all)
- [arXiv: System Call Emulation](https://arxiv.org/search/?query=System%20Call%20Emulation&searchtype=all)
- [From Hack to Elaborate Technique — A Survey on Binary Rewriting](https://doi.org/10.1145/3316415)

## Project Status

n2np is an experimental project. Its supported APIs, runtime behavior, and compatibility limitations depend on the current implementation.