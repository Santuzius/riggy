# Animation adaptation process

How a clip from an animation pack is made to fit an avatar, one clip at a time, and how each step is checked before the user looks at it. [animation-checks.md](animation-checks.md) lists the pairs done so far.

## Principles

- Fixes go into the retargeting or into riggy's own motion, never into a single clip: every clip of a pack, and every pack with the same skeleton, takes the same fix.
- What differs between characters is calibrated per pack and avatar from the pack's neutral clip (named by `neutral` in the bone map) and kept in `<pack>/calibration/<avatar>.toml`, not set by hand per avatar; a hand-set file (`auto = false`) is for packs without a neutral clip.
- riggy's own motion must not add to a clip's: each part of the idle motion gives way where a clip moves the same bones.

## Steps for one clip and avatar

1. Pick the simplest clip not yet checked, one with a complete skeleton so stick figures can be drawn.
2. Check that the installed clip itself is sound: its stick figure (`write_sticks`, grey in `tools/clip_sticks.py`) stands upright on the floor, faces +Z and moves as the clip's name says. A broken clip points at `riggy install`.
3. Off-screen comparison (`RIGGY_CLIP=… nix develop -c cargo test --bin riggy -- --ignored write_sticks --nocapture`):
   - limb bones point where the clip's do (direction errors below 1°, except bones placed beside their parent, whose offsets follow the proportions),
   - hands turn about their axes as the clip's (twist below 1°),
   - in the neutral clip the trunk keeps the avatar's posture (below 0.5°) and the arms hang as the avatar's (below 1°),
   - stick figures from the front, the side and above (`tools/clip_sticks.py`).
4. In the app (`tools/clip_shots.sh`, with `TURN=90` and `TURN=-90` for the sides), compared with the same shots without a clip (`no_such_clip`):
   - posture: trunk upright as without a clip, not leaning back onto the heels or slouching,
   - feet planted where the stance puts them, legs under the body,
   - arms and hands clear of body and clothes, fingers in place,
   - eyes looking ahead (clips leave the eye bones to riggy),
   - the logged pose (`RIGGY_POSE_LOG`, written by the script) through `tools/clip_balance.py`: body centre over the feet and trunk lean close to the shots without a clip.
5. Find the cause of any difference before fixing it: the clip, the retargeting, the calibration, or riggy's own motion on top (idle, stance). Earlier fixes are checked too.
6. Add the pair's regression test (`check(clip, vrm)` in `src/animation/clips/sticks.rs`) and its row in animation-checks.md.
7. Add the clip to `approved` in the default calibration `assets/animations/calibration/<pack>/<avatar>.toml` and the user's copy, so the tray lists it.
8. The user looks at it in `riggy-tray`; commit once they confirm.
