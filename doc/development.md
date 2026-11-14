# Development tooling

## Running

```bash
nix develop -c cargo run                                     # defaults from the checkout, your settings from ~/.config/riggy
RIGGY_CONFIG=$PWD/temp/my.toml nix develop -c cargo run      # other settings laid over the defaults
RIGGY_MODEL=path/to/model.glb nix develop -c cargo run       # another model with the configured mapping
```

riggy re-reads its config and the avatar's mapping file about once a second while it runs:

| File | Applied live | Needs a restart |
|---|---|---|
| `riggy.toml` (defaults) and `~/.config/riggy/riggy.toml` | `[idle]`, `[blink]`, `[mouth]`, `[lipsync]`, `[debug]` | `[avatar]`, `[window]`, `[speech]` |
| mapping (`~/.local/share/riggy/avatars/*.toml`) | `[rest]`, `[expressions.*]`, `[[props]]` | `scale`, `[humanoid]`, `[parts]` |

The dev shell sets `RIGGY_DATA_DIR` to the checkout, so riggy takes its defaults from the repository's `riggy.toml` and `assets/avatars/`; edits to `riggy.toml` apply while riggy runs. The mapping file riggy reads is the copy in `~/.local/share/riggy/avatars/`: tune that one, then copy it to `assets/avatars/` to make it the default.

So poses, mouth shapes and props can be tuned in the editor while the window shows the result.

## Debug inputs

| Input | Effect |
|---|---|
| Mouse wheel | zoom between the full figure and the face |
| `1`–`5` | hold one mouth shape: `aa`, `ih`, `ou`, `ee`, `oh` |
| `0` | mouth back to rest |
| Left mouse button | drag the window to another place (while click-through is off) |
| `F2` | toggle click-through: clicks go to the windows underneath |
| `F3` | cycle lip sync through `sound`, `rhubarb` and `text` (next utterance) |
| `F1` | print the model's node tree with avatar-space joint positions to the log |

- `RIGGY_DUMP_NODES=1` prints the same node tree once right after loading, without a key press. Positions are in model units (mm for the cyborg girl) in avatar space: +Y up, +Z the way the figure faces, +X its left. Use them to choose pivots and translation directions for the mapping file.
- `[debug] expressions = { blink = 0.0, aa = 1.0 }` in the config forces expression weights over everything blink and mouth code set; an empty table turns it off. Forcing `ee` does not raise the cheeks, which the mouth code drives through the separate expression `cheeks`; force that too to see both.
- `[window] zoom` sets the start zoom (0 = full figure, 1 = face).

## Tray and MCP server

```bash
nix develop -c cargo build --bins                  # riggy, riggy-tray, riggy-mcp in target/debug
nix develop -c target/debug/riggy-tray             # starts target/debug/riggy itself
tools/tray_menu.sh                                 # prints the tray menu with its entry ids
tools/tray_menu.sh "Show Riggy"                    # clicks an entry, as the panel would
```

- The tray's log is riggy's log too, since it starts the avatar as its child; `command: …` lines show what arrived on the control socket.
- A second tray only prints that one runs; quit the installed tray first, or give the test one its own `XDG_CONFIG_HOME` and `XDG_RUNTIME_DIR`.
- `riggy-mcp` can be tried without an MCP client by piping JSON-RPC lines into it (the dev shell has ffmpeg):
  ```bash
  (printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"speak","arguments":{"text":"Hello.","language":"English"}}}'; sleep 8) | nix develop -c target/debug/riggy-mcp
  ```
  The `sleep` keeps the pipe open: `riggy-mcp` stops playing when its input ends.

## Screenshots

Screenshots never capture the real desktop. `agent-screenshot` (from portable-nixos, `services/agent/screenshot`) starts riggy on a private virtual X screen and saves exactly its window; the NVIDIA GPU renders there through Vulkan as on the desktop.

```bash
CMD="RIGGY_CONFIG=temp/my.toml nix develop -c target/debug/riggy" GRAB=root agent-screenshot temp/shot.png 'sleep 15'
```

- Build first (`nix develop -c cargo build`); `CMD` runs the binary without building.
- `GRAB=root` is needed: a direct window grab of the Vulkan window comes out black.
- `'sleep 15'` gives the model time to load and the camera time to frame it.
- Runs must be sequential; parallel runs collide on the virtual display.

`tools/expression_shots.sh OUTDIR [EXPRESSION ...]` builds on this: it shoots the face close-up at rest and with each expression at full weight (default: the five mouth shapes), with idle motion and blinking off, and puts the mouth regions side by side in `OUTDIR/all.png`. One shot takes about 20 s.

Shots during speech need the audio to start at a known moment. xdotool's `exec --sync` step starts the sender and waits; with `WAIT=0` the shot follows right after:

```bash
WAIT=0 CMD="RIGGY_CONFIG=temp/my.toml nix develop -c target/debug/riggy" GRAB=root agent-screenshot temp/talk.png 'sleep 10' "exec --sync sh -c 'RIGGY_SOCKET=/tmp/riggy-test.sock target/debug/examples/say --file temp/speech.pcm & sleep 3.05'"
```

Point `[speech] socket` in `temp/my.toml` at the same test socket so a running riggy on the desktop is not disturbed. The sound plays on the real speakers. Prebuffering delays playback by 0.2 s, so aim the sleep at a loud stretch found with the `lipsync` example below, plus 0.2 s.

## Speech and lip sync

- `examples/speech_server_child.rs` starts the speech server as the tray does without a systemd unit, waits until it answers and stops it; stop the `qwentts` unit first and put qwentts-nix's `bin` on `PATH`.
- `riggy say TEXT` speaks a text as `riggy-mcp` does. `examples/say.rs` sends speech to the socket directly: a text through Qwen3-TTS, or with `--file` a raw PCM file (24 kHz s16le mono, the server's `pcm` format). `RIGGY_SOCKET` sets the socket.
- A PCM file for repeatable tests: `curl -s -X POST http://127.0.0.1:8902/v1/audio/speech -H 'Content-Type: application/json' -d '{"voice":"serena","input":"See. Me. Tea.","language":"English","response_format":"pcm","seed":2}' -o temp/ee.pcm`
- `examples/lipsync.rs` runs the lip-sync analysis on PCM files without playing them. Per file it prints the median measures of the voiced windows and the share of each mouth shape; `--frames` prints every 10 ms window. The thresholds in `src/lipsync/sound.rs` come from such runs over one test word list per vowel.
- `[mouth]` tunes the motion live: `time_constant` and `release_time` set how fast the mouth opens and closes, `shape_time` how fast it changes from one shape to another (quiet syllables change it less), `cheek_time` how slowly the cheeks follow the spread shapes, `pause_opening` and `pause_hold` how far it stays open in short pauses between words and for how long.
- `[lipsync] cue_time` and `cue_cheek_time` set how fast mouth and cheeks follow Rhubarb's shapes (modes `rhubarb` and `text`); `[mouth] cheek_time` applies in sound mode only.
- `[lipsync.cues]` in `riggy.toml` replaces the mouth shape for any Rhubarb shape while riggy runs, e.g. `B = { ee = 0.85 }`.
- F3 cycles through the lip-sync modes of the build for the next utterance; the log confirms the mode. In `rhubarb` mode the log shows the analysis time per piece; at the end of speech the log shows how much of it cues drove (the rest fell back to the sound-driven mode).
- The text mode's alignment can be inspected without riggy running: an ignored test prints eSpeak NG's phonemes, the voiced frames of both speeches, the timed phones and the mouth shapes for a recording, e.g.
  ```bash
  RIGGY_PCM=$PWD/temp/speech.pcm RIGGY_TEXT="Schöne Grüße aus Köln." RIGGY_LANGUAGE=German cargo test prints_the_alignment -- --ignored --nocapture
  ```
  Its constants are in `src/lipsync/text/` (`LOOKAHEAD`, `MIN_PAUSE`, eSpeak NG's `RATE`, the cost weights in `align.rs`). IPA symbols without a mouth shape are logged as a warning; add them to `src/lipsync/text/ipa.rs`.
- Cargo features: `rhubarb` (the Rhubarb mode) and `espeak` (the text mode, links libespeak-ng) are on by default; `cargo check --no-default-features` checks the build without them.
- `nix build .#rhubarb` builds the packaged Rhubarb alone, e.g. to try it on a WAV file: `rhubarb -r phonetic -f json speech.wav`.
- The log shows `speech started` and `speech ended` from the lip-sync clock, and `audio output: … Hz` once the sound device is open.

## Model preparation

- `riggy install IN.glb` gives the bone nodes of a model like the Fab cyborg girl a rest pose (`src/install/rest_pose.rs`); [avatars.md](avatars.md) describes the mapping file.
- Models stay in `~/.local/share/riggy/models/`, outside the repository; their licences do not allow redistribution.

## Mapping file features for models with gaps

- `[rest]` corrects the model's rest pose permanently, with deltas like an expression's. The cyborg girl's lips are slightly apart in the model; `[rest]` closes the jaw by 1.5°.
- `[[props]]` attaches simple shapes (sphere or cube, coloured, optionally unlit) to a bone. The cyborg girl has no mouth interior: two dark unlit ellipsoids (upper half on the head, lower on the jaw) form the cavity, two white ones the rows of teeth.
