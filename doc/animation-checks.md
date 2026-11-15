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
- `[TURN=90] tools/clip_shots.sh OUTDIR <clip> [model] [seconds ...]` takes screenshots of the app playing the clip on a virtual screen and logs the posed bones (`RIGGY_POSE_LOG`).
- `tools/clip_balance.py CSV ...` prints body centre over the feet, trunk lean and heel height of stick or pose-log CSVs.
- `nix develop -c cargo test -- --ignored` runs the regression tests of all checked pairs.
