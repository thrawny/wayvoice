{
  description = "wayvoice: voice-to-text daemon for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
      cargoToml = nixpkgs.lib.importTOML ./Cargo.toml;
      inherit (cargoToml.package) version;
    in
    {
      packages = forEachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;

          wayvoice = pkgs.rustPlatform.buildRustPackage {
            pname = "wayvoice";
            inherit version;

            src = lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.makeWrapper
            ];

            buildInputs = [
              pkgs.gtk4
              pkgs.gtk4-layer-shell
            ];

            postInstall = ''
              wrapProgram $out/bin/wayvoice \
                --prefix PATH : ${lib.makeBinPath [
                  pkgs.pipewire
                  pkgs.wtype
                  pkgs.wl-clipboard
                  pkgs.libnotify
                ]}
            '';

            meta = {
              description = "Voice-to-text daemon for Wayland";
              license = lib.licenses.mit;
              mainProgram = "wayvoice";
              platforms = lib.platforms.linux;
            };
          };

        in
        {
          inherit wayvoice;
          default = wayvoice;
        }
      );

      apps = forEachSystem (
        system:
        {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/wayvoice";
          };
          wayvoice = {
            type = "app";
            program = "${self.packages.${system}.wayvoice}/bin/wayvoice";
          };
        }
      );

      devShells = forEachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              pkg-config
              just
              watchexec
              gtk4
              gtk4-layer-shell
            ];
          };
        }
      );
    };
}
