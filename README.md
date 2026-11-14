# riggy

A desktop mascot for Linux written in Rust with [Bevy](https://bevy.org): a rigged 3D figure that idles (breathing, blinking, slight sway) and moves its mouth in sync with speech from a local text-to-speech server.

## Run

```bash
nix develop        # or `direnv allow` once
cargo run
```

The Rust toolchain comes from rustup (`rust-toolchain.toml`); the Nix shell adds Bevy's native libraries (Vulkan, Wayland, X11, ALSA, udev) and the tools of the lip-sync modes (Rhubarb Lip Sync, eSpeak NG).

The flake also builds riggy as a package with three programs: the avatar `riggy`, the tray icon `riggy-tray` and the speech MCP server `riggy-mcp`. Run the avatar and the tray from the checkout, where `riggy.toml` and `assets/` are:

| Package | Cargo features | Lip-sync modes | License of the binary |
|---|---|---|---|
| `nix run .` | `rhubarb`, `espeak` | sound, rhubarb, text | GPL-3.0-or-later |
| `nix run .#riggy-mit` | `rhubarb` | sound, rhubarb | MIT |
| `nix run .#riggy-minimal` | none | sound | MIT |

riggy's source is MIT-licensed. The `espeak` feature links libespeak-ng, which is GPL-3.0-or-later, so a binary built with it is GPL-3.0-or-later as a whole. `src/lipsync/rhubarb_animation/` is a port of Rhubarb Lip Sync's animation rules under their MIT license (`LICENSE` in that folder).

## Model

Models are not part of the repository. The default config expects `assets/models/cyborg_girl.glb`. The Fab download of this model stores its skeleton only in the skins' inverse bind matrices, so its bone nodes need a rest pose before they can be animated:

```bash
python3 tools/fix_bind_pose.py ~/Downloads/cyborg_girl/cyborg_girl.glb assets/models/cyborg_girl.glb
```

`python3` with NumPy is part of the Nix shell.

## Speech

riggy listens on the Unix socket `$XDG_RUNTIME_DIR/riggy.sock`, plays what it receives on the default sound output and moves the mouth with it. One connection carries one utterance:

1. one line of JSON: `{"text": "Hello", "language": "English", "rate": 24000}` (`rate` in Hz; all fields optional),
2. raw PCM, signed 16-bit little-endian mono, until the sender closes the connection.

Later connections wait until the earlier ones have been read. To try it with the local Qwen3-TTS server (`TTS_URL`, default `http://127.0.0.1:8902`):

```bash
nix develop -c cargo run --example say -- --language English "Hello, I am riggy."
```

Three lip-sync modes, chosen with `[lipsync] mode` in `riggy.toml` or cycled with F3 while running:

- `sound` (default): the mouth follows the sound itself. Loudness sets how far it opens, and the balance of low and high frequencies chooses between the five mouth shapes. Speech starts at once. `[mouth]` sets the loudness range, the smoothing and an extra delay of the mouth behind the sound.
- `rhubarb`: [Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync) recognises the sounds and returns mouth shapes with timings, including closed lips for m, b and p. riggy cuts the arriving speech at pauses and analyses each piece (about 1 s per piece), so speech starts about 2 s later. The Nix shell provides `rhubarb`; the flake packages its Linux release.
- `text`: the mouth shapes come from the text in the socket header. [eSpeak NG](https://github.com/espeak-ng/espeak-ng) speaks the text itself, which gives its phonemes in any of its languages, including the German umlauts. riggy matches that synthetic speech to the real one while it arrives (dynamic time warping of MFCCs, as the forced aligner [aeneas](https://github.com/readbeyond/aeneas) does), which times each phoneme, and turns the timed phonemes into mouth shapes with Rhubarb's animation rules. Speech starts about 0.3 s later.

## Speech for AI agents

`riggy-mcp` is a stdio MCP server with one tool, `speak(text, language)`, that reads a short text aloud and returns at once. It fetches the speech from the Qwen3-TTS server (`TTS_URL`), changes its tempo with ffmpeg and sends it to riggy's socket, so the agent's replies come out of riggy's mouth; while riggy is closed, `pw-play` plays it. Texts are queued, each plays to its end, and at least 3 s of silence separate them. Register it, for example, with

```bash
claude mcp add --scope user tts riggy-mcp
```

`RIGGY_SOCKET` points it at another speech socket.

## Tray icon

`riggy-tray` puts an icon in the system tray (StatusNotifierItem, as on KDE Plasma) that controls speech and the avatar:

- **Speech**: on or off. Off stops the speech server's systemd user unit `qwentts`, which frees its GPU memory, and makes `speak` play nothing.
- **Voice** and **Tempo**: apply from the next spoken text on.
- **Show riggy**: starts or closes the avatar on the desktop (a left click on the icon does the same). The tray starts it with `RIGGY_DESKTOP=1`: transparent and above the other windows, whatever `riggy.toml` says.
- **Click-through**: off lets riggy be dragged with the left mouse button; on lets clicks go to the windows underneath.
- **Lip sync**: the mode from the next utterance on.

The choices are kept in `~/.local/state/riggy/settings.toml`; while speech is off, the file `~/.local/state/riggy/speech-off` exists, which the `qwentts` unit can check (`ConditionPathExists`) so that speech stays off across logins. The tray reaches the avatar through the control socket `$XDG_RUNTIME_DIR/riggy-control.sock`, one JSON command per line: `{"click_through":true}`, `{"lipsync":"text"}`, `"quit"`.

## On the desktop

Without the tray, these settings in `riggy.toml` make riggy stand on the desktop as a figure without a window around it:

```toml
[window]
transparent = true      # no border, see-through background
always_on_top = true    # X11 only; on Wayland the KWin script below does it
click_through = true    # clicks go to the windows underneath
```

F2 switches click-through on and off while riggy has the keyboard focus. While click-through is off, dragging with the left mouse button moves the figure.

On KDE Plasma the KWin script in `kwin/riggy/` places riggy on the top edge of the bottom panel at its right end, keeps it above other windows and out of the taskbar, the pager and Alt+Tab, and shows it on all virtual desktops. Install and enable it once:

```bash
kpackagetool6 --type KWin/Script --install kwin/riggy      # --upgrade after changes
kwriteconfig6 --file kwinrc --group Plugins --key riggyEnabled true
qdbus org.kde.KWin /KWin reconfigure
```

The script matches the window by its Wayland app ID (X11 class) `riggy`.

## Development

[doc/development.md](doc/development.md) covers live reloading, debug keys, node dumps, screenshots and lip-sync tuning.
