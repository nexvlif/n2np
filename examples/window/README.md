# n2np window smoke test

`window.exe` is a hand-built x86_64 Windows PE that registers a window class,
creates a top-level window, paints two lines of text with GDI, and runs the
classic `GetMessage`/`TranslateMessage`/`DispatchMessage` loop.

It imports only:

- `kernel32.dll!ExitProcess`
- `user32.dll!RegisterClassA`, `CreateWindowExA`, `ShowWindow`, `UpdateWindow`,
  `GetMessageA`, `TranslateMessage`, `DispatchMessageA`, `DefWindowProcA`,
  `BeginPaint`, `EndPaint`, `DestroyWindow`, `PostQuitMessage`
- `gdi32.dll!TextOutA`

Generate it with:

```bash
python3 tools/make_window_pe.py
```

Run it on Linux/Wayland:

```bash
N2NP_GUI_AUTOCLOSE_MS=1500 cargo run -p n2np-runtime -- \
  --run --target examples/window/window.exe
```

Unset `N2NP_GUI_AUTOCLOSE_MS` to close the window interactively with a click, a
key press, or the compositor close button.
