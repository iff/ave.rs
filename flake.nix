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

      deploy = pkgs.writeScriptBin "deploy"
        ''
          #!/usr/bin/env zsh
          set -eux -o pipefail

          PROJECT_ID=all-o-stasis
          CLOUD_RUN_SERVICE_NAME=api-dev
          MAILEROO_API_KEY=$(op read "op://personal/maileroo boulderapp/credential" --no-newline)

          # TODO: try skopeo and imageId (see cruel world)
          nix build
          TAG=$(docker load < result | awk '{print $3}')
          IMAGE=europe-west1-docker.pkg.dev/$PROJECT_ID/all-o-stasis/api:dev
          docker tag $TAG $IMAGE
          docker push $IMAGE

          gcloud --project $PROJECT_ID run deploy $CLOUD_RUN_SERVICE_NAME --image=$IMAGE --region=europe-west1 \
            --set-env-vars MAILEROO_API_KEY=$MAILEROO_API_KEY,FIRESTORE_DATABASE_ID=dev-db
        '';

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
          deploy
          rustToolchain
          openssl
          pkg-config
          cargo-deny
          cargo-edit
          cargo-watch
          rust-analyzer
          (google-cloud-sdk.withExtraComponents [ google-cloud-sdk.components.gke-gcloud-auth-plugin google-cloud-sdk.components.log-streaming ])
        ];

        shellHook = ''
          ${pkgs.rustToolchain}/bin/cargo --version
        '';
      };
    });
}
