#!/bin/sh
# The probe `omt ssh` runs, verbatim.
#
# One ssh exec, not two: the common case ("omt is already there") costs nothing,
# and a host with MFA prompts once rather than twice.
exec ssh "$@" -- 'omt serve --stdio --proto 1 2>/dev/null || \
  { printf "OMT-MISSING "; uname -sm; (ldd --version 2>&1 | head -1) || echo musl; }'
