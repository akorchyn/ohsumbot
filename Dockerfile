FROM debian:latest AS runtime
WORKDIR /app
RUN apt update -y && apt install -y ca-certificates
COPY target/release/ohsumbot /usr/local/bin
ENTRYPOINT ["/usr/local/bin/ohsumbot"]