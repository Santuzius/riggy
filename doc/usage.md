# Using Riggy

## Tray menu

A left click on the tray icon shows or closes Riggy. The menu:

| Entry | What it does |
|---|---|
| Speech | switches speech on or off. Off stops the speech server, which frees its GPU memory, and makes `speak` play nothing; it stays off across logins. The server is the systemd user unit `qwentts` where the NixOS module installs it; otherwise the tray runs it (`tts-server`) as its child on port 8902. |
| Voice, Tempo | apply from the next spoken text on |
| Show Riggy | starts or closes the avatar on the desktop |
| Avatar | the installed avatars (see [avatars.md](avatars.md)); Riggy restarts with the chosen one |
| Lip sync | the method from the next utterance on (see below) |
| Size | window size; Riggy restarts with it |
| Click-through | on: clicks go to the windows underneath; off: drag Riggy with the left mouse button to move it, with the right one to turn it, and zoom with the mouse wheel between the whole figure and the face. Riggy keeps the turn and zoom for its next start. |
| Start automatically | at login (an entry in `~/.config/autostart/`), and when an AI agent starts `riggy-mcp` |
| Advanced settings › Default MCP instructions | on: `riggy-mcp` tells the agent when and how to speak (the text `riggy-mcp --help` prints); off: it leaves that to your own `CLAUDE.md` or `AGENTS.md`. Applies when an agent next starts `riggy-mcp`. |
| Advanced settings › files | open the settings file, the avatar's mapping file or the avatars folder in your default application |
| Reset to defaults | puts the settings and mapping files back, keeping the replaced files as `*.old` |
| Quit | closes the tray and Riggy |

Only one tray runs at a time; the start menu entry does nothing while it runs.

## Lip sync methods

- **From the sound** (default): the mouth follows the sound itself. Loudness sets how far it opens, and the balance of low and high frequencies chooses between five mouth shapes. Speech starts at once.
- **Rhubarb**: [Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync) recognises the sounds and returns mouth shapes with timings, including closed lips for m, b and p. Riggy cuts the arriving speech at pauses and analyses each piece (about 1 s per piece), so speech starts about 2 s later.
- **From the text**: the mouth shapes come from the spoken text. [eSpeak NG](https://github.com/espeak-ng/espeak-ng) speaks the text itself, which gives its phonemes in any of its languages, including the German umlauts. Riggy matches that synthetic speech to the real one while it arrives (dynamic time warping of MFCCs, as the forced aligner [aeneas](https://github.com/readbeyond/aeneas) does), which times each phoneme, and turns the timed phonemes into mouth shapes with Rhubarb's animation rules. Speech starts about 0.3 s later.

`[mouth]` and `[lipsync]` in the settings tune them; F3 cycles through them while Riggy has the keyboard focus.

## Files

| Path | What |
|---|---|
| `~/.config/riggy/riggy.toml` | your settings: only what differs from the defaults in [riggy.toml](../riggy.toml), whose comments explain every setting |
| `~/.config/riggy/settings.toml` | the choices made in the tray menu |
| `~/.config/riggy/speech-off` | exists while speech is switched off |
| `~/.local/share/riggy/models/NAME.glb` | the avatars |
| `~/.local/share/riggy/avatars/NAME.toml` | the mapping file of avatar `NAME` |
| `~/.local/share/qwentts/models/` | the speech server's models |

Riggy creates its settings and the default avatar's mapping file at its first start. Changes to `[idle]`, `[blink]`, `[mouth]` and `[lipsync]` apply while Riggy runs; the others after a restart ("Show Riggy" off and on). Started from the tray, Riggy's window is transparent and above the others, and the tray's avatar and size and the last zoom and turn made with the mouse replace `[avatar] name` and the matching `[window]` settings. `RIGGY_CONFIG` names another settings file.

## On the desktop

On KDE Plasma the tray enables the KWin script `riggy` (installed with the package), which places Riggy on the top edge of the bottom panel at its right end, keeps it above other windows and out of the taskbar, the pager and Alt+Tab, and shows it on all virtual desktops. It matches the window by its Wayland app ID (X11 class) `riggy`. Without the package, install it from the checkout:

```bash
kpackagetool6 --type KWin/Script --install kwin/riggy      # --upgrade after changes
kwriteconfig6 --file kwinrc --group Plugins --key riggyEnabled true
qdbus org.kde.KWin /KWin reconfigure
```

On X11, `[window] always_on_top` keeps Riggy above the other windows without the script.

## NixOS module

`services.riggy.enable` installs Riggy and runs the speech server from [qwentts-nix](https://github.com/Santuzius/qwentts-nix) on port 8902. `services.riggy.package` picks another build (e.g. `riggy-mit`); the speech server's hardware settings stay with the machine:

```nix
services.qwentts.device = "Vulkan0";
```
