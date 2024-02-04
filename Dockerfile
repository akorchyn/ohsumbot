FROM debian:latest AS runtime
WORKDIR /app
RUN apt update -y && apt install -y ca-certificates libsqlite3-dev ffmpeg pkg-config libmp3lame-dev \
    && apt-get clean
COPY target/release/ohsumbot /usr/local/bin
ENTRYPOINT ["/usr/local/bin/ohsumbot"]
