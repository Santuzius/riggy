# riggy

A desktop mascot for Linux written in Rust with [Bevy](https://bevy.org): a rigged 3D figure that idles (breathing, blinking, slight sway) and moves its mouth in sync with speech from a local text-to-speech server.

## Run

```bash
nix develop        # or `direnv allow` once
cargo run
```

The Rust toolchain comes from rustup (`rust-toolchain.toml`); the Nix shell adds Bevy's native libraries (Vulkan, Wayland, X11, ALSA, udev) and the tools of the lip-sync modes (Rhubarb Lip Sync, eSpeak NG).

The flake also builds riggy as a package; run it from the checkout, where `riggy.toml` and `assets/` are:

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

Later connections wait until the earlier ones have been read. The `speak` tool of [mcp-tts](https://github.com/Santuzius/portable-nixos/tree/main/services/mcp-tts) uses this socket whenever riggy runs, so spoken agent replies come out of riggy's mouth. To try it with the local Qwen3-TTS server (`TTS_URL`, default `http://127.0.0.1:8902`):

```bash
nix develop -c cargo run --example say -- --language English "Hello, I am riggy."
```

Three lip-sync modes, chosen with `[lipsync] mode` in `riggy.toml` or cycled with F3 while running:

- `sound` (default): the mouth follows the sound itself. Loudness sets how far it opens, and the balance of low and high frequencies chooses between the five mouth shapes. Speech starts at once. `[mouth]` sets the loudness range, the smoothing and an extra delay of the mouth behind the sound.
- `rhubarb`: [Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync) recognises the sounds and returns mouth shapes with timings, including closed lips for m, b and p. riggy cuts the arriving speech at pauses and analyses each piece (about 1 s per piece), so speech starts about 2 s later. The Nix shell provides `rhubarb`; the flake packages its Linux release.
- `text`: the mouth shapes come from the text in the socket header. [eSpeak NG](https://github.com/espeak-ng/espeak-ng) speaks the text itself, which gives its phonemes in any of its languages, including the German umlauts. riggy matches that synthetic speech to the real one while it arrives (dynamic time warping of MFCCs, as the forced aligner [aeneas](https://github.com/readbeyond/aeneas) does), which times each phoneme, and turns the timed phonemes into mouth shapes with Rhubarb's animation rules. Speech starts about 0.3 s later.

## Development

[doc/development.md](doc/development.md) covers live reloading, debug keys, node dumps, screenshots and lip-sync tuning.
