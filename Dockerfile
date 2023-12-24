FROM debian:latest AS runtime
WORKDIR /app
RUN apt update -y && apt install -y ca-certificates
COPY /app/target/release/osumbot /usr/local/bin
ENTRYPOINT ["/usr/local/bin/osumbot"]