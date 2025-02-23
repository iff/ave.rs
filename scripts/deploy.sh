#!/usr/bin/env nix-shell
#!nix-shell -i zsh -p skopeo -p google-cloud-sdk

set -eux -o pipefail

PROJECT_ID=all-o-stasis
SERVICE=api
CLOUD_RUN_SERVICE_NAME=$SERVICE

# TAG=$(nix eval --raw -f ./services/$SERVICE/default.nix container.imageTag)
# ARCHIVE=$(nix build --no-sandbox -f ./services/$SERVICE/default.nix container --no-link --print-out-paths)

# TODO build and get TAG
# nix build
# docker load < result
TAG=3d9mi20xpvvhswfp8p7fwpq24f3kfvn3
# IMAGE=gcr.io/$PROJECT_ID/$SERVICE:$TAG
IMAGE=europe-west1-docker.pkg.dev/$PROJECT_ID/all-o-stasis/api:dev
docker tag api:$TAG $IMAGE
docker push $IMAGE

# src=docker-archive://$ARCHIVE
# dst=docker://$IMAGE
#
# skopeo copy --insecure-policy "$src" "$dst"

gcloud --project $PROJECT_ID run deploy $CLOUD_RUN_SERVICE_NAME --image=$IMAGE --region=europe-west1
