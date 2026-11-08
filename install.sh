#!/bin/bash

set -e

# Keep this entry point's build, messages, working directory, and ignored argv.
source "$(dirname "${BASH_SOURCE[0]}")/scripts/install-lifecycle.sh"
ask_install_lifecycle install script
