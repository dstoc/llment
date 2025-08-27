This is a container configuration for use with [mcp-shell](../crates/mcp-shell).
It preinstalls tools useful for rust, python and node development.

## Build
`$ ./build`

## Start
`$ podman-compose up -d`

## Volumes
`./workspace` is mounted **read/write** at `/home/user/workspace`

Replace `./workspace` with a symlink to the path you wish to mount.

## Networking
Currently network is enabled by default. To disable:
* Before starting, uncomment `network_mode: none` in [docker-compose.yml](./docker-compose.yml)
* Or, after starting, run `podman network disconnect sandbox_default sandbox`
