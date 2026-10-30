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

Run the executable on Linux:

```bash
cargo run -p n2np-runtime -- \
  --run --target examples/hello/hello.exe
```

Profiles are optional for execution. Pass `--profile profiles/hello-auto.json` to have the runtime report which called imports have implementations. Add `--trace-out profiles/hello-runtime.json` (or set `N2NP_TRACE=1`) to record the APIs the process actually calls:

```bash
cargo run -p n2np-runtime -- \
  --run --target examples/hello/hello.exe \
  --profile profiles/hello-auto.json \
  --trace-out profiles/hello-runtime.json
```

List the currently implemented API surface:

```bash
cargo run -p n2np-runtime -- --list-apis
```

Additional `--dll-path <dir>` arguments extend the DLL search path. The loader always searches the executable directory, the working directory, `N2NP_DLL_PATH`, and the standard `n2np`/Wine library directories.

> **Note:** Runtime compatibility depends on the APIs and executable features currently supported by n2np. Unsupported imports resolve to a diagnostic thunk that logs the missing function.

### Console smoke test

`examples/hello/hello.exe` is a hand-built console PE64 binary that imports `kernel32.dll!GetStdHandle`, `WriteFile`, and `ExitProcess`. It prints `hello from n2np` and exits:

```bash
python3 tools/make_hello.py
cargo run -p n2np-runtime -- --run --target examples/hello/hello.exe
```

### GUI smoke test (WSLg)

`examples/gui/gui.exe` is a hand-built Windows PE64 GUI binary that imports only `user32.dll!MessageBoxA` and `kernel32.dll!ExitProcess`. On Linux with a working X11/WSLg display, n2np loads the PE, maps it at its preferred image base, resolves the imports to native thunks, and renders the message box through X11:

```bash
python3 tools/make_gui_pe.py

N2NP_GUI_AUTOCLOSE_MS=1500 cargo run -p n2np-runtime -- \
  --run --target examples/gui/gui.exe
```

Unset `N2NP_GUI_AUTOCLOSE_MS` to close the window manually with a click, a key press, or the window close button.

### Registry smoke test

`examples/registry/registry.exe` imports `advapi32.dll!RegCreateKeyExW`, `RegSetValueExW`, `RegQueryValueExW`, and `RegCloseKey`. It writes a string value under `HKCU\Software\N2NP\RegistryTest` and reads it back:

```bash
python3 tools/make_registry_pe.py
N2NP_REGISTRY=/tmp/n2np-registry.json \
  cargo run -p n2np-runtime -- --run --target examples/registry/registry.exe
```

The registry is persisted as JSON at `$N2NP_REGISTRY`, or `$HOME/.n2np/registry.json` when unset. Set `N2NP_REGISTRY=:memory:` to keep it in memory only.

### Current implementation

- PE32+ parsing: headers, sections, imports, delay imports, exports, base relocations, and TLS directories.
- Loader: dependency resolution, mapping, relocations, import/delay-import thunks, TLS callbacks, and `DllMain` attach/detach.
- Runtime: handles and waitable objects, critical sections, virtual memory and heaps, module registry, ANSI/UTF-16 conversion, last-error state, and profile-guided tracing.
- APIs: a growing `kernel32` surface (files, paths, modules, processes, threads, synchronization, memory, time, strings), `user32` message boxes and basic metrics/window APIs, `ntdll` memory/error helpers, `msvcrt` allocation/string/process helpers, and `advapi32` registry keys and values (ANSI and wide, including enumeration and persistence).
- Known gaps: broad GDI/COM/shell support, full thread scheduling, security descriptors, and ordinal export resolution.
- The registry is stored as JSON at `$HOME/.n2np/registry.json`. Set `N2NP_REGISTRY` to another path, or to `:memory:` to disable persistence.


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