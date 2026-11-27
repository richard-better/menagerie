# Builds the agent from source on the machine itself, then runs it on Home Assistant's
# base image. REF is the git ref of github.com/richard-better/menagerie to build.
ARG BUILD_FROM
FROM rust:1-alpine AS build
ARG REF=main
RUN apk add --no-cache musl-dev git
# The ref's current commit, fetched on every build: when it changes, so does this layer,
# and the clone below runs again instead of coming from the build cache.
ADD https://api.github.com/repos/richard-better/menagerie/commits/$REF /tmp/commit.json
RUN git clone https://github.com/richard-better/menagerie /src \
    && git -C /src checkout "$REF"
WORKDIR /src
RUN cargo build --release --locked -p menagerie-agent

FROM $BUILD_FROM
COPY --from=build /src/target/release/menagerie-agent /usr/bin/menagerie-agent
COPY run.sh /run.sh
RUN chmod +x /run.sh
CMD ["/run.sh"]
