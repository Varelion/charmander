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
- **Matches your terminal** — picks up colors and font from your default
  terminal's config, so it doesn't look foreign next to it. See
  [Theming](#theming).
- **Emoji/symbol glyph fallback** — falls back to `Symbola` / `Noto Sans
  Symbols` for codepoints outside your main font's coverage, so names don't
  render as blank boxes.
- Small, resizable, borderless popup window — fits on small screens.
- Closes itself on `Esc` or when it loses focus, so it never lingers.

## Install

```sh
cargo install charmander
```

For the widest glyph coverage, install some symbol fonts (optional — on
Debian/Ubuntu: `apt install fonts-symbola fonts-noto-core`). Fonts are
resolved by family name through fontconfig, so no paths are hardcoded.

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

## Theming

On startup charmander reads the colors and font from your default terminal,
so it blends in rather than looking like a foreign dialog. Supported:

| Terminal | Source |
|----------|--------|
| Alacritty | `~/.config/alacritty/alacritty.toml` |
| kitty | `~/.config/kitty/kitty.conf` |
| xfce4-terminal | `~/.config/xfce4/terminal/terminalrc` |
| GNOME Terminal | its dconf profile, via `gsettings` |

Which one is "default" comes from `$TERMINAL`, then XFCE's
`helpers.rc`, then the `x-terminal-emulator` alternative. If that terminal
has no usable config, the others are tried before falling back to built-in
defaults.

To override any of it, edit `~/.config/charmander/theme.conf` — written on
first run with the detected values, commented out:

```ini
background = "#12081a"
foreground = "#ffa04d"
accent = "#ff7518"
dim = "#4a3b52"
font_family = "FiraCode Nerd Font Mono"
```

Any key you leave out keeps its auto-detected value. `dim` is the badge
fill, `accent` is the selection/cursor color.

## Files

| Path | Purpose |
|------|---------|
| `~/.config/charmander/theme.conf` | Color/font overrides |
| `~/.config/charmander/favorites.txt` | Starred codepoints, one hex per line |
| `~/.config/charmander/default_view.txt` | `all` or `favorites` |

On Windows these live under `%APPDATA%\charmander\`; `$XDG_CONFIG_HOME` is
respected where set.

## Data

Character names come from the Unicode Character Database
(`UnicodeData.txt`, Unicode, Inc.), used under the [Unicode License
Agreement](https://www.unicode.org/license.txt).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
at your option.
