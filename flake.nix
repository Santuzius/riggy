{
  description = "riggy: a Bevy desktop mascot that idles and lip-syncs to TTS";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      # Libraries Bevy opens at runtime with dlopen (Vulkan, Wayland, X11 cursor/input).
      runtimeLibs = with pkgs; [
        vulkan-loader
        wayland
        libxkbcommon
        libx11
        libxcursor
        libxi
        libxrandr
        alsa-lib
        udev
      ];
    in
    {
      # The Rust toolchain comes from the user's rustup (pinned by rust-toolchain.toml); the shell only adds Bevy's native dependencies.
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = runtimeLibs;
        packages = with pkgs; [ espeak-ng vulkan-tools (python3.withPackages (p: [ p.numpy ])) ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
      };
    };
}
