#!/usr/bin/env bash
# Wrapper: run steghide via Docker from the host. Enables bench comparisons on
# macOS-arm64 where steghide is no longer in Homebrew.
#
# Docker image already has steghide as the entrypoint; pass args straight through.
# Mounts:
#   /workdir      = $PWD (for CLI relative paths)
#   /var/folders  = /var/folders (macOS temp)
#   /tmp          = /tmp          (alt temp)
set -euo pipefail
exec docker run --rm --platform linux/amd64 \
  -v "$PWD":/workdir -w /workdir \
  -v /var/folders:/var/folders \
  -v /tmp:/tmp \
  rsteg-tools:steghide "$@"
