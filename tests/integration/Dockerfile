# The integration suite, with a real omt and a real git.
#
# The build happens here rather than being copied in, so what is tested is what
# this tree produces on a machine that is not the author's.
FROM rust:1.90-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build -p omt --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-venv git ca-certificates procps \
 && rm -rf /var/lib/apt/lists/*

# A git identity, because the git capabilities run against a real repository and
# `git commit` refuses without one.
RUN git config --global user.email "tests@example.invalid" \
 && git config --global user.name "omt integration tests" \
 && git config --global init.defaultBranch main \
 && git config --global --add safe.directory '*'

WORKDIR /omt
COPY --from=build /src/target/debug/omt /usr/local/bin/omt
COPY tests/integration /omt/tests/integration
# The two generated artefacts the parity tests read. Copied rather than skipped:
# "the web client can reach every capability" is exactly the check that stops a
# feature being implemented, tested and unreachable.
COPY web/src/capabilities.ts /omt/web/src/capabilities.ts
COPY docs/reference/capabilities.md /omt/docs/reference/capabilities.md
RUN python3 -m venv /venv && /venv/bin/pip install --no-cache-dir \
      -r /omt/tests/integration/requirements.txt

ENV OMT_BINARY=/usr/local/bin/omt
WORKDIR /omt/tests/integration
CMD ["/venv/bin/python", "-m", "pytest", "-q"]
