# Animation checks

Which clip has been checked on which avatar. How a pair is checked: [animation-adaptation-process.md](animation-adaptation-process.md). Each checked pair has a regression test in `src/animation/clips/sticks.rs`.

|Clip|Avatar|Status|Test|
|---|---|---|---|
|`motifect_locomotion_complete_v1_0_fbx/idle_neutral`|`cyber_ver101` (VRM)|fixed|`idle_neutral_on_cyber_ver101`|
|`motifect_locomotion_complete_v1_0_fbx/idle_neutral`|`cyborg_girl` (GLB)|shoulders fixed, to be checked in the app|`idle_neutral_on_cyborg_girl`|
|`animation_pack_basic_motions/idle`|`cyber_ver101` (VRM)|checked|`basic_motions_idle_on_cyber_ver101`|
|`animation_pack_basic_motions/idle`|`cyborg_girl` (GLB)|checked|`basic_motions_idle_on_cyborg_girl`|
|`animation_pack_basic_motions/turn_left`|`cyber_ver101` (VRM)|checked|`basic_motions_turn_left_on_cyber_ver101`|
|`animation_pack_basic_motions/turn_left`|`cyborg_girl` (GLB)|checked|`basic_motions_turn_left_on_cyborg_girl`|
|`animation_pack_basic_motions/turn_right`|`cyber_ver101` (VRM)|checked|`basic_motions_turn_right_on_cyber_ver101`|
|`animation_pack_basic_motions/turn_right`|`cyborg_girl` (GLB)|checked|`basic_motions_turn_right_on_cyborg_girl`|

## Tools

- `RIGGY_CLIP=<clip> RIGGY_VRM=<model.vrm> nix develop -c cargo test --bin riggy -- --ignored write_sticks --nocapture` prints each bone's largest direction and twist error and writes `temp/sticks/<clip>.{clip,avatar}.csv`.
- `tools/clip_sticks.py temp/sticks/<clip> out.png` draws the clip (grey) and the avatar (colour) over each other (needs matplotlib).
- `[TURN=90] [HOLD=1] [LEGS=1] [NO_CONTACT=1] [STEP=0.033] tools/clip_shots.sh OUTDIR <clip> [model] [seconds ...]` takes screenshots of the app playing the clip on a virtual screen and logs the posed bones (`RIGGY_POSE_LOG`). With `HOLD=1` each shot shows the clip frozen at that second, so it shows exactly a frame the collision report names; `LEGS=1` lets the clip move the legs; `NO_CONTACT=1` lets the arms go through the body, for comparing; `STEP` sets how often the pose is logged.
- Collision report (`src/animation/clips/collisions.rs`, body shapes in `src/clips/body.rs`): each humanoid bone's part of the avatar mesh becomes a shape, and every frame lists where a part goes into another one that is not its neighbour, beyond how deep it lies in the rest pose. Spring-bone parts (hair, coat skirts, bust) are left out; hands and feet are checked only with their points. `idle_neutral` of the Motifect locomotion pack is the reference: depths up to its own on the same pair of parts count as acceptable. The legs follow the clip as the user's legs switch (`clip_legs`) says, or `RIGGY_LEGS=0`/`1`; otherwise they stay in the rest pose. With `RIGGY_CLIP=<clip> RIGGY_VRM=<model>`:
  - `cargo test --release --bin riggy -- --ignored collision_report --nocapture` lists the clip's colliding pairs, deepest first, with the second of the deepest frame and the time ranges;
  - `... collision_survey` gives every installed clip's worst collisions beyond the reference;
  - the report adapts the clip to the avatar's body and lands the arms on it as riggy does (`src/animation/clips/clearance.rs`); `RIGGY_CLEAR=0` shows the clip without that;
  - `... contact_smoothness` plays the clip with the contact rule as riggy smooths it and prints the largest frame-to-frame jumps it adds to the hands and fingertips (`RIGGY_AT=<from>-<to>` lists the corrections and contacts in that range);
  - `... app_collisions` with `RIGGY_POSE_CSV=<pose log>` checks the poses the app logged (rotations included), so fades and riggy's own motion count;
  - `... explain_collision` with `RIGGY_PARTS=<part>,<part> RIGGY_AT=<seconds>` prints each point of the first part inside the second at that second;
  - `... clearing_arm_hang` finds, per arm, the smallest swing at the shoulder (`actor.arm_hang` in the pack's calibration) that clears the clip's arm collisions;
  - `... print_parts` prints each part's shape size.
- `tools/clip_balance.py CSV ...` prints body centre over the feet, trunk lean and heel height of stick or pose-log CSVs.
- `nix develop -c cargo test -- --ignored` runs the regression tests of all checked pairs.
