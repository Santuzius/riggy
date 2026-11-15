# Unity humanoid muscles to bone rotations

Ported from [unity-asset-reader](https://github.com/fatal10110/unity-asset-reader)'s `packages/animation/src/humanoid.ts` and `muscle.ts` (commit 226687d, MIT, © 2026 fatal10110; see `LICENSE`). `muscle.ts` itself ports names from AssetStudio's `MuscleHelper.cs` (MIT, © Perfare, RazTools, Razviar).

`rig.json` is that project's test rigs, built by the Unity editor 6000.6.4f1: the body of `humanoid` (a T-pose skeleton) with the fingers of `humanoid2`, each finger's first bone carried into `humanoid`'s hand frame, all with Unity's default muscle limits and twist shares. `riggy-rig.ts` wrote it: copied into `packages/animation/tests/` of a clone of that repository and run with `npx tsx tests/riggy-rig.ts rig.json DEFAULTS`, where `DEFAULTS` is an AssetBundle holding an Avatar with Unity's default limits (any humanoid imported with default muscle settings).
