# n2np GUI smoke test

`gui.exe` is a hand-built x86_64 Windows PE that imports only:

- `user32.dll!MessageBoxA`
- `kernel32.dll!ExitProcess`

It is used to verify the PE loader, the Win64 thunk ABI, and the Wayland GUI backend.

Generate it with:

```bash
python3 tools/make_gui_pe.py
```

Run it on Linux/Wayland:

```bash
N2NP_GUI_AUTOCLOSE_MS=1500 cargo run -p n2np-runtime -- \
  --run --target examples/gui/gui.exe
```

Unset `N2NP_GUI_AUTOCLOSE_MS` to interactively close the window with a click or key press.
