FROM debian:bookworm-slim AS ytdlp

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp \
    -o /usr/local/bin/yt-dlp \
    && chmod 0755 /usr/local/bin/yt-dlp

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
ENV YTDLP_BINARY_PATH=yt-dlp
ENV FFMPEG_BINARY_PATH=ffmpeg
ENV TEMP_DIR=/tmp/videodownloaderbackend

CMD ["videodownloaderbackend"]
