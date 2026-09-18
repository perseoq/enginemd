FROM rust:1-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates
COPY css ./css
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd -m -u 1000 enginemd
COPY --from=build /app/target/release/enginemd /usr/local/bin/enginemd
USER enginemd
EXPOSE 10300
ENTRYPOINT ["enginemd"]
