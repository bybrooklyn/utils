<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo/logo-dark.svg">
    <img alt="lealand" src="logo/logo-light.svg" width="420">
  </picture>
</p>

lealand is an eza-style listing you can walk around in. You run it as `ll`. It draws under the prompt instead of taking over the terminal, and when you quit, your shell is in the folder you ended up in.

Typing two letters quickly runs something: `cl` opens Claude Code where you are, `fi` opens Dolphin. Claude Opus 5.5 wrote it.

```
 lealand   ~ › utils   4 changed           ? help
▎ cwpl/                2 items  3h ago
  dotfiles/            4 items  8d ago    M
  lealand/             8 items  2m ago    M
  install.sh              1.1k  3d ago       exec
  id_ed25519               411  1y ago       private
```

The listing is only as wide as its longest name. The last column is empty unless something is unusual about the file for the folder it is in: `exec`, `private` (only you can read it), `read-only` in a folder you can write to, `world-writable`, a broken link, or an owner that is neither you nor the folder's owner. The letter before it is git status. Dotfiles are shown, dimmed, after everything else.

## Keys

- **← →** go up a folder, or open the folder or file. Enter also opens.
- **↑ ↓** move one, **ctrl ↑ ↓** move five. **pgup pgdn home end** do what they say.
- **backspace** goes back to the folder you were in before.
- **/** jumps to any folder. Type part of its name and press tab.
- **esc esc** quits and leaves the shell where it was.

## Chords

Each key of a chord has to come within 300 ms of the last one. When a chord is also the start of a longer one, like `mk` and `mkcl`, it waits those 300 ms for the next key before it runs.

- **qq** quit and cd to where you are
- **cl** quit, cd, and run `claude`
- **fi** open Dolphin here (ll stays open)
- **code** open VS Code here
- **tt** open a new Ghostty tab here
- **fu** fullscreen, with a preview of the file or folder under the cursor, and back
- **op** open the file with a different app
- **mk** make a folder. Type its name in the top line and press Enter
- **mkcl** make a folder, cd into it and run `claude`
- **rm** move the file or folder to the trash, after a y/n
- **?** list all of this

Add or change chords in `~/.config/lealand/chords`, one per line:

```
gi    exit  lazygit
cl    exit  claude --continue
ko    spawn konsole
code  none
```

`mkdir` and `mkdir-exit <command>` ask for a folder name, and `trash` asks before moving things to the trash. `spawn` runs a command in the current folder with no terminal, for apps with their own window, and ll stays open. `exit` quits and runs the command in your shell after the cd, so terminal programs go there. `none` removes a chord. A `spawn` command gets the file under the cursor as `$f`.

## Jumping

`/` knows every folder under `~` except hidden ones (`~/.config` is the exception), anything a `.gitignore` covers (the same rules tokei and soem use), and build or cache folders like `node_modules`, `target`, `go/pkg` and Wine prefixes. Typos are fine: `/dwonlods` finds Downloads. The rest of the best match shows greyed out as you type, like zsh-autosuggestions.

Folders you go to often and recently rank higher. That comes from every `cd` in zsh (a hook in the dotfiles zshrc), every folder you open in ll, and on the first run, the `cd`s in `~/.zsh_history`.

With nothing typed yet, the list is the folders you go to most, so `/` then Tab takes you to the top one. You get 2 seconds after `/` and after the first letter, then 500 ms between letters. Running out cancels. Tab or Enter goes, ↑ ↓ picks another match.

## Opening files

The first time you open a type of file, ll lists the apps that handle it, with KDE's default on top and every other app below. Type to filter. It remembers the pick for that MIME type. `op` asks again and remembers the new answer. Terminal apps like micro run in place and ll comes back when they exit.

## Starting up

It opens with a short animation: a few lines on what it read, then the willow, which dissolves into the listing. It takes about half a second, and any key skips it and still does what that key does. `ll --disable-flag` turns it off for good and `ll --enable-flag` brings it back. It also skips itself in terminals under 76 columns or 25 rows.

## Install

```sh
bash ~/utils/lealand/install.sh
```

It needs cargo, a Nerd Font for the icons, and a terminal with 24-bit colour. The `ll` zsh function does the cd, so it has to be in `~/.zshrc`. The dotfiles zshrc has it, and the installer prints the block if yours doesn't.

## Files

| file | what |
|---|---|
| `~/.config/lealand/chords` | your chords |
| `~/.config/lealand/apps` | the app picked for each MIME type |
| `~/.config/lealand/no-flag` | there when the launch animation is off |
| `~/.local/share/lealand/visits` | where you've been, for ranking `/` |
| `~/.cache/lealand/dirs` | the folder index, rebuilt in the background when it's over an hour old |

To remove it: `cargo uninstall lealand`, delete those files, and take the block starting `# lealand (~/utils/lealand)` out of the zshrc.

The wordmark is set in [Instrument Serif](https://github.com/Instrument/instrument-serif) Italic (SIL Open Font License) with a willow drawn in code. `logo/make.py` makes both SVGs and the terminal version.
