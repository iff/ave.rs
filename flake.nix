{
  description = "ave.rs";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      flake-utils,
      nixpkgs,
      rust-overlay,
    }:

    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [
          (import rust-overlay)
          (self: super: {
            rustToolchain = pkgs.symlinkJoin {
              name = "rust-toolchain";
              paths = [
                (super.rust-bin.stable.latest.minimal.override {
                  extensions = [
                    "clippy"
                    "rust-docs"
                    # "rust-src"
                  ];
                })
                (super.rust-bin.selectLatestNightlyWith (toolchain: toolchain.rustfmt))
              ];
            };
          })
        ];

        pkgs = import nixpkgs { inherit system overlays; };

        deploy = pkgs.writeScriptBin "deploy" ''
          #!/usr/bin/env zsh
          set -eux -o pipefail

          PROJECT_ID=all-o-stasis
          IMAGE=europe-west1-docker.pkg.dev/$PROJECT_ID/all-o-stasis/api:dev

          nix build
          ${pkgs.skopeo}/bin/skopeo copy \
            --dest-creds=oauth2accesstoken:$(gcloud auth print-access-token) \
            docker-archive:result \
            docker://$IMAGE

          CLOUD_RUN_SERVICE_NAME=api-dev
          MAILEROO_API_KEY=$(op read "op://personal/maileroo boulderapp/credential" --no-newline)

          gcloud --project $PROJECT_ID run deploy $CLOUD_RUN_SERVICE_NAME --image=$IMAGE --region=europe-west1 \
            --set-env-vars MAILEROO_API_KEY=$MAILEROO_API_KEY,FIRESTORE_DATABASE_ID=dev-db
        '';

        app = pkgs.rustPlatform.buildRustPackage {
          pname = "all-o-stasis";
          version = "0.0.1";
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type: (pkgs.lib.cleanSourceFilter path type) && (builtins.baseNameOf path != "target");
          };

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          GIT_HASH = self.rev or "dirty";

          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.git
          ];
          buildInputs = [ pkgs.libgit2 ];
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
        packages = {
          default = container;
          container = container;
          app = app;
          imageId = pkgs.writeTextFile {
            name = "image-id";
            text = "${container.imageTag}\n";
          };
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            deploy
            rustToolchain
            openssl
            pkg-config
            cargo-deny
            cargo-edit
            cargo-watch
            rust-analyzer
            (google-cloud-sdk.withExtraComponents [
              google-cloud-sdk.components.gke-gcloud-auth-plugin
              google-cloud-sdk.components.log-streaming
            ])
            pinact
          ];

          shellHook = ''
            ${pkgs.rustToolchain}/bin/cargo --version
          '';
        };
      }
    );
}
