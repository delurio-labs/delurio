# The image: the static `delurio` binary (musl) on an empty base, for linux/amd64 and linux/arm64. The release
# workflow builds the binaries and puts them in image/ as delurio-<arch> before `docker buildx build`; the checker
# reads graph files and nothing else, so the image holds nothing else: no shell, no package manager, no runtime.
FROM scratch
ARG TARGETARCH
COPY image/delurio-${TARGETARCH} /opt/delurio/delurio
COPY schemas /opt/delurio/schemas
COPY SPEC.md NOTICE LICENSE /opt/delurio/
WORKDIR /work
USER 65532:65532
ENTRYPOINT ["/opt/delurio/delurio"]
