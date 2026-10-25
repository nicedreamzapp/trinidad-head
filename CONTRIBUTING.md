# Contributing

Thanks for taking a look. Trinidad Head runs on Windows and Mac, and I can't test every setup, so a
bug report from your machine is useful even if you never send code.

## Reporting a problem

Open an [issue](https://github.com/nicedreamzapp/trinidad-head/issues/new/choose). The things that
make it quick to fix:

- Windows 10 or 11, or your macOS version and whether the Mac is Apple Silicon or Intel
- whether you used a release download or built it yourself (and from which commit)
- your shell and what was running in the window, for example Claude Code or another full-screen app
- what you typed or clicked, and a screenshot if it is a drawing problem

## Running it locally

On a Mac:

```bash
scripts/build-mac-app.sh              # puts "Trinidad Head" in ~/Applications
open -n -a "Trinidad Head"
```

Without an Apple Development certificate, run `TH_ALLOW_ADHOC=1 scripts/build-mac-app.sh`.

On Windows (add llvm-mingw to PATH first):

```powershell
rustup default stable-x86_64-pc-windows-gnullvm
cargo build --release
target\release\trinidad-head.exe
```

## Tests

```bash
cargo test --workspace
```

The suite needs no window or GPU, and CI runs it on Linux, Mac and Windows for every push and pull
request. The tests that drive the real app window are separate: `scripts/mac-selftest.sh` on a Mac
and `pc-tools/th_selftest.ps1` on Windows.

## Pull requests

Small and focused is easiest to review. For big changes, open an issue first so we can talk it
through. Say which OS you tested on. Unless you say otherwise, your contribution is dual-licensed
under MIT or Apache 2.0, like the rest of the project.

Everyone taking part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md).
