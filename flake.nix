{
  description = "generic rust env";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    { self
    , flake-utils
    , nixpkgs
    , rust-overlay
    }:

    flake-utils.lib.eachDefaultSystem (system:
    let
      overlays = [
        (import rust-overlay)
        (self: super: {
          rustToolchain =
            let
              rust = super.rust-bin;
            in
            if builtins.pathExists ./rust-toolchain.toml then
              rust.fromRustupToolchainFile ./rust-toolchain.toml
            else if builtins.pathExists ./rust-toolchain then
              rust.fromRustupToolchainFile ./rust-toolchain
            else
              rust.stable.latest.default;
        })
      ];

      pkgs = import nixpkgs { inherit system overlays; };

      app = pkgs.rustPlatform.buildRustPackage {
        pname = "all-o-stasis";
        version = "0.0.1";
        src = ./.;

        cargoLock = {
          lockFile = ./Cargo.lock;
        };

        nativeBuildInputs = [ pkgs.pkg-config ];
        # TODO needed here?
        PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
      };

      container = pkgs.dockerTools.buildLayeredImage {
        name = "api";

        config = {
          Env = [
            "PROJECT_ID=all-o-stasis"
            "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" # for firestore
          ];
          Cmd = [ "${app}/bin/all-o-stasis" ];
          ExposedPorts = {
            "8080/tcp" = { };
          };
        };
      };
    in
    {
      # TODO expose container.imageId
      defaultPackage = container;

      devShells.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          bruno
          rustToolchain
          openssl
          pkg-config
          cargo-deny
          cargo-edit
          cargo-watch
          rust-analyzer
          (google-cloud-sdk.withExtraComponents [ google-cloud-sdk.components.gke-gcloud-auth-plugin ])
        ] ++ lib.optionals pkgs.stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security darwin.apple_sdk.frameworks.SystemConfiguration ];

        shellHook = ''
          ${pkgs.rustToolchain}/bin/cargo --version
        '';
      };
    });
}
