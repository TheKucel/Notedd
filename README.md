# Notedd

<img src="assets/notedd.svg" width="64" alt="Notedd icon">

I wanted a little notes window I could pull up on Windows while doing something else. So that's what this is. Hit `Ctrl+Alt+Space`, write whatever, hit it again to hide it.

`Ctrl+P` opens the stack of notes. Scroll with the wheel or use the arrow keys to move through it. Start typing to search all your notes. If nothing comes up, press Enter to make a note with what you typed. `Ctrl+F` searches the note you're in.

## Running it

Notedd is for Windows 10 and 11 (x64). Download and run `notedd-setup.exe`. [Releases](https://github.com/TheKucel/Notedd/releases)

The installer puts Notedd in `%LOCALAPPDATA%\Programs\Notedd` by default. You can pick an editor mode, choose where to keep your notes, and decide whether to start with Windows. It runs without admin rights and is unsigned, so Windows may show a warning.

To remove it, use **Settings → Apps** in Windows or run `.\uninstall.exe --uninstall` from the install folder. Your notes stay where they are.

## Shortcuts

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
