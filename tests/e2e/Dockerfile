# The end-to-end fixture: a Linux box with omt built in it, plus the tools the
# tests drive it with. Separate from tests/docker (an ssh *target*) because
# this one runs omt itself and needs a toolchain.
FROM rust:1.90-slim

# git for the diff and worktree tests; the rest is what a shell test needs.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      git ca-certificates procps \
 && rm -rf /var/lib/apt/lists/*

# A predictable identity, so git commits inside the tests do not prompt.
RUN git config --global user.email "e2e@example.com" \
 && git config --global user.name "omt e2e" \
 && git config --global init.defaultBranch main

WORKDIR /omt

# A non-root user, because omt's socket refuses a peer that is not the same
# user and running everything as root would never exercise that.
RUN useradd -m -s /bin/bash omtuser
ENV CARGO_TARGET_DIR=/tmp/target
