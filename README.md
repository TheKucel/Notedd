# Notedd

<img src="assets/notedd.svg" width="64" alt="Notedd icon">

I wanted a little notes window I could pull up on Windows while doing something else. So that's what this is. Hit `Ctrl+Alt+Space`, write whatever, hit it again to hide it.

`Ctrl+P` opens the stack of notes. Scroll with the wheel or use the arrow keys to move through it. Start typing to search all your notes. If nothing comes up, press Enter to make a note with what you typed. `Ctrl+F` searches the note you're in.

## Running it

Notedd is for Windows 10 and 11 (x64). Download and run `notedd-setup.exe`. [Releases](https://github.com/TheKucel/Notedd/releases)

The installer puts Notedd in `%LOCALAPPDATA%\Programs\Notedd` by default. You can pick an editor mode, choose where to keep your notes, and decide whether to start with Windows. It runs without admin rights and is unsigned, so Windows may show a warning.

To remove it, use **Settings → Apps** in Windows or run `.\uninstall.exe --uninstall` from the install folder. Your notes stay where they are.

## macOS

Extract `notedd-setup-macos.zip` and open **Notedd Setup** on macOS 13 or later. The installer uses the same stack of cards as on Windows. Choose your theme, editor, global shortcut, and folders before installing. It supports Apple Silicon and Intel Macs and defaults to `~/Applications` on a fresh install. The setup app has an ad hoc signature and no Apple notarization. [Releases](https://github.com/TheKucel/Notedd/releases)

Notedd runs without an icon in the Dock or menu bar and opens over the current workspace, including full-screen apps. Press `Control+Option+Space` to show or hide it and `Control+Option+N` to create a note from anywhere. Closing the window leaves it running in the background. You can also open Notedd from Applications to show its window again.

Click the colored dot or press `Command+Shift+C` to change a note's color. In the stack, right-click a note to open its palette. Click the note tab or press `Command+P` for the stack, press `Command+F` to find in the current note, and press `Command+Shift+M` to switch between Markdown and Simple. Simple hides the markup while you write and displays pasted images in the note. Select text and right-click to format it.

Drag the note tab or the top edge of the folder to move the window. It slows near a screen edge and snaps into place when released close enough. Open Settings with `Command+,`. The **General** tab lets you choose where the window slides from, set the speed, or turn edge snapping off. Notedd follows the macOS Reduce Motion setting.

Notedd uses `~/Documents/Notedd` by default. To open notes from Windows, copy the Markdown files and their `assets` folder together, then use **Settings → General → Notes folder → Choose…** to open the copy.

To build locally, install Xcode or its Command Line Tools and Rust. Run `rustup target add aarch64-apple-darwin x86_64-apple-darwin` and then `./macos/build.sh` from the repository root. The script produces `Notedd.app`, `Notedd Setup.app`, and the release archive `notedd-setup-macos.zip` in `macos/target`. Windows and macOS share the Rust library for Markdown parsing and note storage.

## Shortcuts

These bindings are for Windows.

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
| `Ctrl+Shift+Delete` or `Delete` in the stack | Delete a note |
| `Ctrl+Z` in the stack | Bring back the last deleted note |
| `Ctrl+,` | Open settings |
| `Ctrl+Q` | Quit |
| `Esc` | Back out of the current view, or hide the editor |
| `Ctrl+=`, `Ctrl+-`, or `Ctrl+wheel` | Change text size. `Ctrl+0` resets it |

You can change the bindings under **Settings → Shortcuts**. Settings also has themes and a language picker for English, Russian, and Simplified Chinese.

## Where the notes go

Each note is a Markdown file. The first line is the title. They go in `Documents\Notedd` by default, and you can pick another folder in Settings. App settings live in `%APPDATA%\Notedd\settings.txt`.

Headings, lists, quotes, code, links and checkboxes render as you type. Click a checkbox or press `Ctrl+Enter` to tick it. `Ctrl+click` opens a link.

Use `==text==` for a highlight, or select something and press `Ctrl+Shift+H`.

Under **Settings → General → Editor**, pick **Markdown** to see the markup or **Simple** to hide it while you write. In Simple mode, select text and right-click to format it. `Ctrl+B` toggles bold, `Ctrl+I` italic, `Ctrl+Shift+S` strikethrough, and `Ctrl+E` inline code. Both modes save the same Markdown files.

You can paste a screenshot or drop an image file into a note. Notedd saves the image in an `assets` folder next to the notes and puts a relative Markdown link in the file. Note colors are stored in the YAML front matter.

Deleted notes go to the Recycle Bin when Windows can put them there. If that fails, Notedd deletes the file directly.

## Building it

It's a Rust app using Win32, Direct2D and RichEdit. You'll need Rust with the Windows MSVC toolchain. The build uses the Windows SDK's `rc.exe` to embed the icon and version info.

Run these from the repo root. The installer needs the app build first because it embeds `notedd.exe`.

```powershell
cargo build --release --locked --manifest-path app/Cargo.toml
cargo build --release --locked --manifest-path setup/Cargo.toml
```

The installer ends up at `setup\target\release\notedd-setup.exe`.

## License

The code is MIT. [License](LICENSE)

Geologica is bundled under the SIL Open Font License 1.1. [Font license](assets/fonts/OFL-Geologica.txt)
