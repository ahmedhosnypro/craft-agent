# The runner simulation image: a container that behaves like a GitHub
# Actions runner consuming the composite action. The release binary and
# the workspace arrive as runtime mounts; no repo source and no secrets
# are baked in.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
  && apt-get install -y --no-install-recommends git gh jq curl ca-certificates \
  && rm -rf /var/lib/apt/lists/*

ENV RUNNER_TEMP=/runner/temp \
    GITHUB_WORKSPACE=/workspace

RUN mkdir -p /runner/temp /workspace /out

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 0755 /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
