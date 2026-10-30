# n2np console smoke test

`hello.exe` is a hand-built x86_64 Windows PE that imports only:

- `kernel32.dll!GetStdHandle`
- `kernel32.dll!WriteFile`
- `kernel32.dll!ExitProcess`

It prints `hello from n2np` to stdout and exits.

Generate it with:

```bash
python3 tools/make_hello.py
```

Run it on Linux:

```bash
cargo run -p n2np-runtime -- --run --target examples/hello/hello.exe
```

Capture a runtime profile:

```bash
cargo run -p n2np-runtime -- --run --target examples/hello/hello.exe \
  --trace-out profiles/hello-runtime.json
```
