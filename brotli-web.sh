#!/bin/bash
cd ./packages/frontend/apps/web/dist && find . -type f ! -name "*.br" ! -name "*.gz" -print0 | parallel -0 -j16 "brotli -kfZ"
