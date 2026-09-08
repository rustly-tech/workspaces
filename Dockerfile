FROM rust:1.88-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p rustly-git-http

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home rustly \
    && install -d -o rustly -g rustly /var/lib/rustly-git
COPY --from=build /src/target/release/rustly-git-http /usr/local/bin/rustly-git-http
USER rustly
ENV RUSTLY_GIT_BIND=0.0.0.0:8090
ENV RUSTLY_GIT_ROOT=/var/lib/rustly-git
EXPOSE 8090
VOLUME ["/var/lib/rustly-git"]
ENTRYPOINT ["/usr/local/bin/rustly-git-http"]

