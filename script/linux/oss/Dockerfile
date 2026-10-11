ARG BASE_IMAGE
FROM ${BASE_IMAGE}
ARG RUST_VERSION
COPY platforms.json /tmp/platforms.json
RUN apt-get update && apt-get install -y --no-install-recommends python3 \
    && python3 -c 'import json,subprocess; p=json.load(open("/tmp/platforms.json"))["ubuntu"]; subprocess.run(["apt-get","install","-y","--no-install-recommends",*p["dependencies"]],check=True)' \
    && rm -rf /var/lib/apt/lists/* /tmp/platforms.json
ENV CARGO_HOME=/opt/cargo RUSTUP_HOME=/opt/rustup PATH=/opt/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup.sh \
    && sh /tmp/rustup.sh -y --profile minimal --default-toolchain "$RUST_VERSION" --component rustfmt,clippy,rust-analyzer \
    && rm /tmp/rustup.sh \
    && chmod -R a+rX /opt/cargo /opt/rustup
