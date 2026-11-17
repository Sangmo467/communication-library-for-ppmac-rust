FROM debian:sid-slim
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates curl xz-utils git make cmake gcc g++ bison gawk python3 \
    gcc-15-loongarch64-linux-gnu g++-15-loongarch64-linux-gnu libc6-dev-loong64-cross \
    qemu-user libicu-dev libssl-dev libkrb5-3 zlib1g && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
ENV PATH=/cargo/bin:/opt/pwsh:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ENV CARGO_HOME=/cargo RUSTUP_HOME=/rustup CARGO_BUILD_JOBS=2
ENTRYPOINT ["/opt/pwsh/pwsh", "-NoProfile", "-File", "scripts/Build-LoongArchNative.ps1"]
