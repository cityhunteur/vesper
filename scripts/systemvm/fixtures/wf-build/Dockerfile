# Build fixture for the developer workflow suite (GH #31): two layers on the pinned test image.
ARG BASE=alpine:3.22
FROM ${BASE}
RUN echo built-in-system-vm > /built.txt
COPY payload.txt /payload.txt
CMD ["sh", "-c", "cat /built.txt /payload.txt"]
