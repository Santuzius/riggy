# riggy

A desktop mascot for Linux written in Rust with [Bevy](https://bevy.org): a rigged 3D figure that idles (breathing, blinking, slight sway) and moves its mouth in sync with speech from a local text-to-speech server.

## Run

```bash
nix develop        # or `direnv allow` once
cargo run
```

The Rust toolchain comes from rustup (`rust-toolchain.toml`); the Nix shell adds Bevy's native libraries (Vulkan, Wayland, X11, ALSA, udev).

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

Two lip-sync modes, chosen with `[lipsync] mode` in `riggy.toml` or switched with F3 while running:

- `sound` (default): the mouth follows the sound itself. Loudness sets how far it opens, and the balance of low and high frequencies chooses between the five mouth shapes. Speech starts at once. `[mouth]` sets the loudness range, the smoothing and an extra delay of the mouth behind the sound.
- `rhubarb`: [Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync) recognises the sounds and returns mouth shapes with timings, including closed lips for m, b and p. riggy cuts the arriving speech at pauses and analyses each piece (about 1 s per piece), so speech starts about 2 s later. The Nix shell provides `rhubarb`; the flake packages its Linux release.

## Development

[doc/development.md](doc/development.md) covers live reloading, debug keys, node dumps, screenshots and lip-sync tuning.
