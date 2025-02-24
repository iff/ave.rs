#!/usr/bin/env zsh
set -eux -o pipefail

PROJECT_ID=all-o-stasis
SERVICE=api
CLOUD_RUN_SERVICE_NAME=$SERVICE

# TODO: try skopeo and imageId (see cruel world)
nix build
TAG=$(docker load < result | awk '{print $3}')
IMAGE=europe-west1-docker.pkg.dev/$PROJECT_ID/all-o-stasis/api:dev
docker tag $TAG $IMAGE
docker push $IMAGE

gcloud --project $PROJECT_ID run deploy $CLOUD_RUN_SERVICE_NAME --image=$IMAGE --region=europe-west1
