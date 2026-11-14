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
      lib = pkgs.lib;
      cargoToml = lib.importTOML ./Cargo.toml;
      # riggy with a choice of Cargo features: "rhubarb" puts the Rhubarb program on riggy's PATH, "espeak" links libespeak-ng (GPL-3.0-or-later, so that build is GPL-3.0-or-later as a whole). Run it from the riggy checkout, where riggy.toml and assets/ are.
      mkRiggy = features: pkgs.rustPlatform.buildRustPackage {
        pname = "riggy";
        inherit (cargoToml.package) version;
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./examples ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        buildNoDefaultFeatures = true;
        buildFeatures = features;
        nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper ];
        buildInputs = runtimeLibs ++ lib.optional (lib.elem "espeak" features) pkgs.espeak-ng;
        # The unit tests run in the dev shell; the package build only compiles.
        doCheck = false;
        postFixup = ''
          wrapProgram $out/bin/riggy \
            --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
            ${lib.optionalString (lib.elem "rhubarb" features) "--prefix PATH : ${rhubarb}/bin"}
        '';
        meta = {
          inherit (cargoToml.package) description;
          license = if lib.elem "espeak" features then lib.licenses.gpl3Plus else lib.licenses.mit;
          mainProgram = "riggy";
        };
      };
    in
    {
      packages.${system} = {
        inherit rhubarb;
        # All lip-sync modes.
        default = mkRiggy [ "rhubarb" "espeak" ];
        # Only the sound and Rhubarb modes; MIT throughout.
        riggy-mit = mkRiggy [ "rhubarb" ];
        # Only the sound-driven lip sync.
        riggy-minimal = mkRiggy [ ];
      };

      # The Rust toolchain comes from the user's rustup (pinned by rust-toolchain.toml); the shell only adds Bevy's native dependencies.
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = [ pkgs.pkg-config ];
        # espeak-ng: linked by the `espeak` feature (text-driven lip sync).
        buildInputs = runtimeLibs ++ [ pkgs.espeak-ng ];
        packages = with pkgs; [ rhubarb espeak-ng vulkan-tools (python3.withPackages (p: [ p.numpy ])) ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs ++ [ pkgs.espeak-ng ]);
      };
    };
}
