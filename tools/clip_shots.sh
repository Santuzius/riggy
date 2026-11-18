#!/usr/bin/env bash
# Screenshots of the avatar while it plays one installed clip, asked for over the control socket as the tray does, on a virtual X screen (agent-screenshot from portable-nixos).
# Usage: tools/clip_shots.sh OUTDIR CLIP [MODEL] [SECONDS ...]   (default model cyber_ver101, shots 1 2 4 s after the clip starts)
# HOLD=1 shows the clip frozen at each SECONDS instead (`hold` command), so a shot shows exactly that frame of the clip, as the collision report gives it.
# STEP=seconds logs the pose that often (default 0.1).
# NO_CONTACT=1 lets the arms go through the body (riggy's contact rule off), for comparing.
# LEGS=1 lets the clip move the legs (the legs switch, `clip_legs`).
# TURN=degrees turns the avatar first (positive to its left, e.g. 90 for a side view), as the tray's turn does.
# Writes OUTDIR/<clip>_<s>.pose.csv (every bone's position as posed in the app, see src/animation/pose_log.rs), OUTDIR/<clip>_<s>.png (with TURN: <clip>_<turn>_<s>.png) and OUTDIR/<clip>.log. Riggy's own idle motion runs as configured by default, as in the app.
set -euo pipefail
out=$(realpath -m "${1:?usage: $0 OUTDIR CLIP [MODEL] [SECONDS ...]}")
clip=${2:?clip name}
model=${3:-cyber_ver101}
shift 3 2>/dev/null || shift $#
times=("$@")
[ ${#times[@]} -gt 0 ] || times=(1 2 4)
cd "$(dirname "$0")/.."
mkdir -p "$out"
nix develop -c cargo build -q --bin riggy
run=$(mktemp -d)
cat >"$run/riggy.toml" <<TOML
clips = false
[avatar]
name = "$model"
TOML
stem=${clip//\//_}${TURN:+_$TURN}
stem=${stem// /_}
for t in "${times[@]}"; do
  # The clip is asked for 10 s after the window opens, when the avatar has loaded.
  if [ -n "${HOLD:-}" ]; then ask="{\"hold\":{\"clip\":\"$clip\",\"at\":$t}}"; wait=2; else ask="{\"play\":\"$clip\"}"; wait=$t; fi
  { [ -z "${LEGS:-}" ] || echo '{"clip_legs":true}'; [ -z "${TURN:-}" ] || echo "{\"turn\":$TURN}"; echo "$ask"; } >"$run/play.json"
  CMD="${NO_CONTACT:+RIGGY_NO_CONTACT=1 }RIGGY_POSE_LOG=$out/${stem}_$t.pose.csv RIGGY_POSE_LOG_STEP=${STEP:-0.1} XDG_RUNTIME_DIR=$run RIGGY_CONFIG=$run/riggy.toml nix develop -c target/debug/riggy" \
    GRAB=root LOG="$out/$stem.log" agent-screenshot "$out/${stem}_$t.png" "sleep 10" "exec --sync sh -c 'nc -U -N $run/riggy-control.sock <$run/play.json'" "sleep $wait" | tail -1
done
rm -rf "$run"
