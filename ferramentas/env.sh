#!/usr/bin/env bash
# Source this file from WSL; resolve paths independently of the caller's cwd.
export DEVKITPRO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/devkitpro"
export DEVKITPRO="$(readlink -f -- "$DEVKITPRO")"
export PORTLIBS_PATH="$DEVKITPRO/portlibs"
export PORTLIBS="$PORTLIBS_PATH/switch"
export LIBNX="$DEVKITPRO/libnx"
export DEVKITA64="$DEVKITPRO/devkitA64"
export PATH="$DEVKITA64/bin:$DEVKITPRO/tools/bin:$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="/tmp/horizon-switch-target"
