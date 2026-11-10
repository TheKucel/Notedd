<div align="center">
  <img src="assets/notedd.svg" width="96" alt="Notedd icon">
  <h1>Notedd</h1>
  <p>A small notes window, always close at hand.</p>
  <p><b>English</b> · <a href="README.ru.md">Русский</a></p>
  <p><a href="https://github.com/TheKucel/Notedd/releases">Download</a> · <a href="#shortcuts">Shortcuts</a> · <a href="#building-it">Build from source</a></p>
</div>

---

I wanted a little notes window I could pull up while doing something else. So that's what this is. Hit `Ctrl+Alt+Space`, write whatever, hit it again to hide it.

`Ctrl+P` opens the stack of notes. Scroll with the wheel or use the arrow keys to move through it. Start typing to search all your notes. If nothing comes up, press Enter to make a note with what you typed. `Ctrl+F` searches the note you're in.

## Windows

Notedd runs on Windows 10 and 11 (x64). Download and run `notedd-setup.exe`. [Releases](https://github.com/TheKucel/Notedd/releases)

The installer puts Notedd in `%LOCALAPPDATA%\Programs\Notedd` by default. You can pick an editor mode, choose where to keep your notes, and decide whether to start with Windows. It runs without admin rights and is unsigned, so Windows may show a warning.

To remove it, use **Settings → Apps** in Windows or run `.\uninstall.exe --uninstall` from the install folder. Your notes stay where they are.

## macOS

Download `notedd-setup-macos.zip` from [Releases](https://github.com/TheKucel/Notedd/releases), extract it and open `Notedd Setup.app`; it puts Notedd in `~/Applications` by default. Notedd needs macOS 13 or later and runs on Apple Silicon and Intel Macs. The installer has an ad hoc signature and no Apple notarization, so the first time, right-click it and choose **Open**.

Notedd lives in the menu bar, with no icon in the Dock. The shortcuts below use `Command` where Windows uses `Ctrl`. Settings go in `~/Library/Application Support/Notedd/settings.txt`.

## Linux

Extract `notedd-linux-x86_64.tar.gz` and run `notedd`. It needs an X server: on a Wayland desktop Notedd runs through XWayland, so it can move and dock its window. Set `NOTEDD_WAYLAND=1` to run on Wayland itself; the window then stays where the desktop puts it. The tray icon needs AppIndicator support. Settings go in `~/.config/Notedd/settings.txt`.

## Shortcuts

These bindings are for Windows and Linux. On macOS, `Command` takes the place of `Ctrl` inside the app.

| Keys | What they do |
| --- | --- |
| `Ctrl+Alt+Space` | Show or hide Notedd from anywhere |
| `Ctrl+Alt+N` | New note from anywhere |
| `Ctrl+N` | New note |
| `Ctrl+P` | Open the note stack |
| `Ctrl+F` | Find in the current note |
| `Ctrl+T` | Keep the window on top |
| `Ctrl+Shift+C` | Change the note color |
| `Ctrl+Shift+H` | Toggle highlight on the selected text |
| `Ctrl+Shift+Delete` | Delete a note |
| `Ctrl+Z` in the stack | Bring back the last deleted note |
| `Ctrl+,` | Open settings |
| `Ctrl+Q` | Quit |
| `Esc` | Back out of the current view, or hide the editor |
| `Ctrl+=`, `Ctrl+-`, or `Ctrl+wheel` | Change text size. `Ctrl+0` resets it |

You can change the bindings under **Settings → Shortcuts**. Settings also has themes and a language picker for English, Russian, and Simplified Chinese.

## Where the notes go

Each note is a Markdown file. The first line is the title. They go in `Documents\Notedd` by default, and you can pick another folder in Settings. On Windows, app settings live in `%APPDATA%\Notedd\settings.txt`.

Headings, lists, quotes, code, links and checkboxes render as you type. Click a checkbox or press `Ctrl+Enter` to tick it. `Ctrl+click` opens a link.

Use `==text==` for a highlight, or select something and press `Ctrl+Shift+H`.

Under **Settings → General → Editor**, pick **Markdown** to see the markup or **Simple** to hide it while you write. In Simple mode, select text and right-click to format it. `Ctrl+B` toggles bold, `Ctrl+I` italic, `Ctrl+Shift+S` strikethrough, and `Ctrl+E` inline code. Both modes save the same Markdown files.

You can paste a screenshot or drop an image file into a note. Notedd saves the image in an `assets` folder next to the notes and puts a relative Markdown link in the file. Note colors are stored in the YAML front matter.

Deleted notes go to the Recycle Bin (the Trash on macOS and Linux) when the system can put them there. If that fails, Notedd deletes the file directly.

## Building it

Notedd is a Rust app built on [GPUI](https://www.gpui.rs/), with local patches in `app/vendor` (see `app/vendor/PATCHES.md`). The editor model is in `editor`, Markdown parsing and note storage in `core`.

On Windows you'll need Rust with the MSVC toolchain. The build uses the Windows SDK's `rc.exe` to embed the icon and version info. The installer embeds `notedd.exe`, so build the app first:

```powershell
cargo build --release --locked --manifest-path app/Cargo.toml
cargo build --release --locked --manifest-path setup/Cargo.toml
```

The installer ends up at `setup/target/release/notedd-setup.exe`.

On macOS, install Xcode or its Command Line Tools, run `rustup target add aarch64-apple-darwin x86_64-apple-darwin`, then `./app/macos/bundle.sh`. It produces `Notedd.app`, `Notedd Setup.app` and `notedd-setup-macos.zip` in `app/target/macos`.

On Linux, install the X11, Wayland, xkbcommon, fontconfig, GTK 3 and AppIndicator development packages (the list is in `.github/workflows/build.yml`), then run `cargo build --release --locked --manifest-path app/Cargo.toml`.

## License

The code is MIT. [License](LICENSE)

Geologica is bundled under the SIL Open Font License 1.1. [Font license](assets/fonts/OFL-Geologica.txt)
