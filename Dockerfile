FROM debian:bookworm-slim AS ytdlp

ARG TARGETARCH=amd64

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    case "${TARGETARCH}" in \
        amd64) ytdlp_asset="yt-dlp_linux" ;; \
        arm64) ytdlp_asset="yt-dlp_linux_aarch64" ;; \
        *) echo "unsupported target architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl -L "https://github.com/yt-dlp/yt-dlp/releases/latest/download/${ytdlp_asset}" \
        -o /usr/local/bin/yt-dlp; \
    chmod 0755 /usr/local/bin/yt-dlp; \
    /usr/local/bin/yt-dlp --version

FROM rust:1.95.0-slim-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY migrations ./migrations
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ytdlp /usr/local/bin/yt-dlp /usr/local/bin/yt-dlp
COPY --from=builder /app/target/release/videodownloaderbackend /usr/local/bin/videodownloaderbackend

ENV APP_ENV=production
ENV HOST=0.0.0.0
ENV YTDLP_BINARY_PATH=/usr/local/bin/yt-dlp
ENV FFMPEG_BINARY_PATH=/usr/bin/ffmpeg
ENV TEMP_DIR=/tmp/videodownloaderbackend

CMD ["videodownloaderbackend"]
