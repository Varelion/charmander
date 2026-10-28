# charmander

<img src="assets/icon.png" width="64" height="64" alt="charmander icon">

A hotkey-driven Unicode character picker. Hit a shortcut, search, pick a
character, it gets typed into whatever window had focus — editor, terminal,
Discord client, anywhere.

Inspired by kitty's `unicode_input` kitten, but standalone: no terminal
required, works system-wide.

## Features

- **Search** — live substring search over every named Unicode character.
- **Favorites** — star any character (`Ctrl+F` or right-click). Favorited
  entries show `★`, everything else shows `☆`.
- **Views** — switch between `All` and `★ Favorites` with `Ctrl+1` / `Ctrl+2`.
  `Ctrl+D` saves whichever view is active as the one charmander opens to next
  time.
- **Windows-style Alt codes** — hold `Alt`, type digits (number row or
  numpad, both work), release `Alt`. Resolves through Windows-1252, same as
  real Windows Alt codes (e.g. `Alt+0150` → en dash, `Alt+0151` → em dash).
- **Emoji/symbol glyph fallback** — falls back to `Symbola` / `Noto Sans
  Symbols` for codepoints outside your main font's coverage, so names don't
  render as blank boxes.
- Small, resizable, borderless popup window — fits on small screens.

## Install

```sh
cargo install charmander
```

Needs a font covering box-drawing/symbol glyphs for best results (the
fallback fonts above are optional but recommended — on Debian/Ubuntu:
`apt install fonts-symbola fonts-noto-core`).

## Usage

```sh
charmander --setup
```

Binds `Ctrl+Shift+U` to charmander automatically on:

- **XFCE** — via `xfconf-query`
- **GNOME** — via `gsettings` custom keybindings
- **Windows** — embeds the hotkey in a Start Menu shortcut (the native
  mechanism Windows uses for global shortcuts)

Elsewhere (KDE, macOS, anything else) `--setup` prints the command to bind
manually, since those don't have a keybinding API that's safe to script
blindly.

Then press the hotkey anywhere, search, and hit `Enter`.

| Key               | Action             |
|-------------------|--------------------|
| `Enter`           | Insert character   |
| `↑` / `↓`         | Navigate list      |
| `Ctrl+F`          | Toggle favorite    |
| Right-click       | Toggle favorite    |
| `Alt`+digits      | Windows Alt code   |
| `Ctrl+1` / `Ctrl+2` | Switch view (All / Favorites) |
| `Ctrl+D`          | Set current view as default |
| `Esc`             | Cancel             |

## Data

Character names come from the Unicode Character Database
(`UnicodeData.txt`, Unicode, Inc.), used under the [Unicode License
Agreement](https://www.unicode.org/license.txt).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option.
