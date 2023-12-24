FROM debian:latest AS runtime
WORKDIR /app
RUN apt update -y && apt install -y ca-certificates libsqlite3-dev
COPY target/release/ohsumbot /usr/local/bin
ENTRYPOINT ["/usr/local/bin/ohsumbot"]
