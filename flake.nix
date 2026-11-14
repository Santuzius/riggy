{
  description = "Riggy: an animated desktop mascot for your AI agent that speaks with a self-hosted TTS server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # The speech server riggy-mcp speaks with; the NixOS module runs it.
    qwentts-nix = {
      url = "github:Santuzius/qwentts-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Builds the Cargo dependencies in a derivation of their own, so a change to riggy's source does not compile Bevy again.
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, qwentts-nix, crane, ... }:
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
      speechTools = [ pkgs.ffmpeg pkgs.pipewire pkgs.systemd ];
      # The speech server, which the tray runs where the NixOS module's systemd unit does not.
      qwentts = qwentts-nix.packages.${system}.default;
      craneLib = crane.mkLib pkgs;
      # riggy with a choice of Cargo features: "rhubarb" puts the Rhubarb program on riggy's PATH, "espeak" links libespeak-ng (GPL-3.0-or-later, so that build is GPL-3.0-or-later as a whole). The package holds the avatar `riggy`, the tray icon `riggy-tray` and the speech MCP server `riggy-mcp`.
      mkRiggy = features:
        let
          common = {
            pname = "riggy";
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./examples ./riggy.toml ./assets ./kwin ];
            };
            strictDeps = true;
            cargoExtraArgs = "--locked --no-default-features --features '${lib.concatStringsSep "," features}'";
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = runtimeLibs ++ lib.optional (lib.elem "espeak" features) pkgs.espeak-ng;
            # The unit tests run in the dev shell; the package build only compiles.
            doCheck = false;
          };
          # Bevy and the other dependencies, rebuilt only when Cargo.lock or the features change.
          cargoArtifacts = craneLib.buildDepsOnly common;
        in
        craneLib.buildPackage (common // {
        inherit cargoArtifacts;
        nativeBuildInputs = common.nativeBuildInputs ++ [ pkgs.makeWrapper ];
        # share/riggy holds the defaults ($RIGGY_DATA_DIR: riggy.toml and the mapping files), laid out as in the repository; share/kwin/scripts/riggy the KWin script that places the avatar on the panel; the start menu entry starts the tray.
        postInstall = ''
          mkdir -p $out/share/riggy/assets $out/share/kwin/scripts
          cp riggy.toml $out/share/riggy/
          cp -r assets/avatars $out/share/riggy/assets/
          cp -r kwin/riggy $out/share/kwin/scripts/
          install -Dm644 assets/riggy.desktop $out/share/applications/riggy.desktop
          install -Dm644 assets/icons/tray-tts-avatar.svg $out/share/icons/hicolor/scalable/apps/riggy.svg
        '';
        # Speech (riggy say, riggy-mcp) goes through ffmpeg and, while the avatar is closed, pw-play; the tray switches the speech server with systemctl or runs it (tts-server); xdg-open opens the tray's files; kwriteconfig6 enables the KWin script.
        postFixup = ''
          wrapProgram $out/bin/riggy \
            --set-default RIGGY_DATA_DIR $out/share/riggy \
            --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
            --prefix PATH : ${lib.makeBinPath speechTools} \
            ${lib.optionalString (lib.elem "rhubarb" features) "--prefix PATH : ${rhubarb}/bin"}
          wrapProgram $out/bin/riggy-mcp --prefix PATH : ${lib.makeBinPath speechTools}
          wrapProgram $out/bin/riggy-tray --set-default RIGGY_DATA_DIR $out/share/riggy --prefix PATH : ${lib.makeBinPath [ pkgs.systemd pkgs.xdg-utils pkgs.kdePackages.kconfig qwentts ]}
        '';
        meta = {
          inherit ((lib.importTOML ./Cargo.toml).package) description;
          license = if lib.elem "espeak" features then lib.licenses.gpl3Plus else lib.licenses.mit;
          mainProgram = "riggy";
        };
      });
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

      # NixOS module: installs Riggy (with its start menu entry; the tray adds itself to the login autostart) and runs the speech server (qwentts-nix) for it. The machine adds what depends on its hardware, such as services.qwentts.device.
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.riggy;
        in
        {
          imports = [ qwentts-nix.nixosModules.default ];

          options.services.riggy = {
            enable = lib.mkEnableOption "Riggy, the desktop mascot that speaks the agents' replies, with its tray icon and speech server";
            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              defaultText = lib.literalExpression "riggy.packages.\${system}.default";
              description = "The riggy package (avatar, riggy-tray, riggy-mcp).";
            };
          };

          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];

            # riggy-mcp and riggy-tray expect the server on port 8902.
            services.qwentts = {
              enable = lib.mkDefault true;
              port = lib.mkDefault 8902;
            };
            # Stays off across logins after switching speech off in the tray menu.
            systemd.user.services.qwentts.unitConfig.ConditionPathExists = "!%h/.config/riggy/speech-off";
          };
        };

      # The Rust toolchain comes from the user's rustup (pinned by rust-toolchain.toml); the shell only adds Bevy's native dependencies.
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = [ pkgs.pkg-config ];
        # espeak-ng: linked by the `espeak` feature (text-driven lip sync).
        buildInputs = runtimeLibs ++ [ pkgs.espeak-ng ];
        packages = with pkgs; [ rhubarb espeak-ng vulkan-tools ffmpeg python3 ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs ++ [ pkgs.espeak-ng ]);
        # In the checkout the defaults come from the repository, so editing riggy.toml there applies while riggy runs.
        shellHook = ''
          export RIGGY_DATA_DIR="''${RIGGY_DATA_DIR:-$PWD}"
        '';
      };
    };
}
