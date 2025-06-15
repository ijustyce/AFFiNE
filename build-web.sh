#!/bin/bash
export BUILD_TYPE=stable
yarn affine @affine/web build
if [[ "$OSTYPE" == "darwin"* ]]; then
  echo "Mac platform"
  cd ./packages/frontend/apps/web/dist && sed -i '' 's/\/js\//https:\/\/cdn.kpromise.top\/affine\/js\//g' ./selfhost.html
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
  echo "Linux platform"
  cd ./packages/frontend/apps/web/dist && sed -i 's/\/js\//https:\/\/cdn.kpromise.top\/affine\/js\//g' ./selfhost.html
else
  echo "Unknown platform please change selfhost.html"
fi