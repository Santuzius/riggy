# To do

- TBD: start Riggy together with Zed. Zed starts its MCP servers only when an agent thread starts, so "When an AI agent starts riggy-mcp" does not cover a plain Zed start; a Home Manager option could wrap Zed's start menu entry so it also starts `riggy-tray`.
- TBD: VRM backend. A Cargo feature `vrm` with bevy_vrm1, whose loader fills `Humanoid` from the VRM humanoid and passes the expressions on to bevy_vrm1's expression weights (spring bones and LookAt come with it). VRM 0.x models are converted once to VRM 1.0 with the VRM Add-on for Blender. Done when a VRM model is installed and chosen like a GLB one, and idle, blinking and lip sync work on it.
