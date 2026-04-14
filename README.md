# InstallRS

A Rust-based framework for building self-contained software installers.

Very early development stage - not ready for production use.

Why? Because NSIS is a pain for anything non-trivial.
We can do better in 2026.

## Features

- Write your installer logic in plain Rust
- Full access to Rust's standard library and third-party crates
- Scans your source code to detect which files need to be embedded
- Embeds those files into a self-contained executable using `include_bytes!`
- Provides a simple API for installing and removing files on the target system
- Automatically generates both installer and uninstaller binaries
- Supports file compression (lzma, gzip, bzip2) to reduce binary size
- Small binaries — no runtime overhead

## Unfeatures

- No GUI (yet)
- No advanced installation options (yet)
- No control over exe icons, version info, etc. (yet)

## Usage

Write a library crate with `install` and `uninstall` functions:

```rust
use anyhow::Result;
use installrs::Installer;

pub fn install(i: &mut Installer) -> Result<()> {
    i.set_out_dir("C:/my_app");
    i.dir("assets", "assets")?;
    i.file("app.exe", "app.exe")?;
    i.uninstaller("uninstall.exe")?;
    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    i.remove("C:/my_app")?;
    Ok(())
}
```

Then build with the `installrs` CLI:

```sh
installrs --target ./my-installer --output installer.exe
```

See the `example-rs` directory for a working example.

## Installer API

| Method               | Description                                                  |
| -------------------- | ------------------------------------------------------------ |
| `set_out_dir(dir)`   | Set the base output directory for relative destination paths |
| `set_in_dir(dir)`    | Set the base input directory for relative source paths       |
| `file(src, dest)`    | Install a single embedded file                               |
| `dir(src, dest)`     | Install an embedded directory tree                           |
| `mkdir(dir)`         | Create a directory on the target system                      |
| `uninstaller(dest)`  | Write the uninstaller executable                             |
| `remove(path)`       | Remove a file or directory                                   |
| `exists(path)`       | Check whether a path exists                                  |
| `exec_shell(cmd)`    | Run a shell command                                          |
| `include_file(path)` | Hint to embed a file (no-op at runtime)                      |
| `include_dir(path)`  | Hint to embed a directory (no-op at runtime)                 |

## Command Line Options

```
--target <dir>           Directory containing the installer source crate (default: .)
--output <file>          Output installer file path (default: ./installer)
--compression <method>   lzma, gzip, bzip2, or none (default: lzma)
--ignore <patterns>      Comma-separated glob patterns to ignore (default: .git,.svn,node_modules)
--target-triple <triple> Rust target triple for cross-compilation (e.g. x86_64-pc-windows-gnu)
--verbose                Enable debug output
--quiet                  Suppress non-error output
--silent                 Suppress all output
```

## Requirements

- Rust toolchain (stable)
- The target crate must be a library crate exporting `install` and `uninstall`

## License

This project is licensed under the MIT License.
See the [LICENSE.txt](LICENSE.txt) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
