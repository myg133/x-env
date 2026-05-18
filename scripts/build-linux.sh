#!/bin/bash
# build-linux.sh - 在 Linux 服务器上编译 x-env

set -e

IMAGE="rust:1.75-alpine"
CONTAINER_NAME="x-env-builder"
OUTPUT_DIR="./target/linux-release"

echo "Building x-env for Linux..."

# 创建输出目录
mkdir -p "$OUTPUT_DIR"

# 运行 Docker 容器编译
docker run --rm \
    --name "$CONTAINER_NAME" \
    -v "$(pwd)":/app \
    -w /app \
    "$IMAGE" \
    sh -c "apk add --no-cache musl-dev && cargo build --release --target x86_64-unknown-linux-musl"

# 复制输出
cp target/x86_64-unknown-linux-musl/release/x-env ./"$OUTPUT_DIR/"

echo "Build complete! Output: $OUTPUT_DIR/x-env"
