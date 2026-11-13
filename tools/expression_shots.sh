#!/usr/bin/env bash
# Screenshots of the face close-up with each given expression at full weight, plus one at rest, each on a virtual X screen (agent-screenshot from portable-nixos), so the real desktop is never captured.
# Usage: tools/expression_shots.sh OUTDIR [EXPRESSION ...]   (default: rest aa ih ou ee oh)
# Writes OUTDIR/<name>.png (400x700 window), OUTDIR/<name>.toml (the config used), OUTDIR/<name>.log, and OUTDIR/all.png with the mouth regions side by side.
# Idle motion and blinking are switched off so the shots compare. One shot takes about 20 s; runs are sequential because parallel ones collide on the virtual display.
set -euo pipefail
out=$(realpath -m "${1:?usage: $0 OUTDIR [EXPRESSION ...]}")
shift
names=("$@")
[ ${#names[@]} -gt 0 ] || names=(rest aa ih ou ee oh)
cd "$(dirname "$0")/.."
mkdir -p "$out"
nix develop -c cargo build -q
for name in "${names[@]}"; do
  extra=""
  [ "$name" = rest ] || extra=", $name = 1.0"
  cat >"$out/$name.toml" <<EOF
[window]
zoom = 1.0
[idle]
breath_angle = 0.0
sway_shift = 0.0
sway_angle = 0.0
head_angle = 0.0
[debug]
expressions = { blink = 0.0$extra }
EOF
  CMD="RIGGY_CONFIG=$out/$name.toml nix develop -c target/debug/riggy" GRAB=root LOG="$out/$name.log" agent-screenshot "$out/$name.png" 'sleep 15' | tail -1
done
pngs=()
for name in "${names[@]}"; do pngs+=("$out/$name.png"); done
nix run nixpkgs#imagemagick -- "${pngs[@]}" -crop 140x90+130+460 +repage -resize 200% +append "$out/all.png"
echo "$out/all.png"
