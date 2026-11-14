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
      # Rhubarb Lip Sync (MIT): mouth shapes with timings from recorded speech, for the `rhubarb` lip-sync mode. Not in nixpkgs; the release build is patched for NixOS. It finds its speech models in res/ next to the real executable, so the whole release stays together under share/.
      rhubarb = pkgs.stdenv.mkDerivation rec {
        pname = "rhubarb-lip-sync";
        version = "1.14.0";
        src = pkgs.fetchurl {
          url = "https://github.com/DanielSWolf/rhubarb-lip-sync/releases/download/v${version}/Rhubarb-Lip-Sync-${version}-Linux.zip";
          hash = "sha256-qakHSGLP9HstWbi/OZpnijsLdPlFKtatlMspKRPdhmc=";
        };
        nativeBuildInputs = [ pkgs.unzip pkgs.autoPatchelfHook ];
        buildInputs = [ pkgs.stdenv.cc.cc.lib ];
        installPhase = ''
          mkdir -p $out/share $out/bin
          cp -r . $out/share/rhubarb
          ln -s $out/share/rhubarb/rhubarb $out/bin/rhubarb
        '';
      };
    in
    {
      packages.${system}.rhubarb = rhubarb;

      # The Rust toolchain comes from the user's rustup (pinned by rust-toolchain.toml); the shell only adds Bevy's native dependencies.
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = runtimeLibs;
        packages = with pkgs; [ rhubarb espeak-ng vulkan-tools (python3.withPackages (p: [ p.numpy ])) ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
      };
    };
}
