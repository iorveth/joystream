#!/usr/bin/env bash

set -e

# If OS is supported will install:
#  - build tools and any other dependencies required for rust
#  - rustup - rust insaller
#  - rust compiler and toolchains
#  - skips installing substrate and subkey
curl https://getsubstrate.io -sSf | bash -s -- --fast

