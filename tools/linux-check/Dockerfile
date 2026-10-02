# Pitwall's CI checks on Linux, for a machine that has Docker but no Linux. Same base as the
# ubuntu-22.04 CI runner; builds natively on arm64 or amd64. Use run.sh rather than this directly.
FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive

COPY packages.txt /tmp/packages.txt
# git, ps and lsof come with GitHub's runners and the core tests call them.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates gnupg rsync xz-utils git procps lsof \
      $(sed 's/#.*//' /tmp/packages.txt) \
 && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable --component clippy

# The target dir lives in a named volume (see run.sh), never in the source tree. Fewer jobs and
# line tables only keep linking inside a small Docker VM's memory. SHELL as on GitHub's runners.
ENV CARGO_TARGET_DIR=/cargo-target \
    CARGO_BUILD_JOBS=3 \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=line-tables-only \
    CARGO_PROFILE_TEST_DEBUG=line-tables-only \
    CARGO_TERM_COLOR=never \
    CI=true \
    SHELL=/bin/bash

COPY check.sh /usr/local/bin/pitwall-check
RUN mkdir /src
WORKDIR /work
CMD ["pitwall-check"]
