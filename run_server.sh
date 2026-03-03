#!/bin/bash

# asc_mcp_server 一键运行脚本
# 作者: dpiner
# 日期: $(date +%Y-%m-%d)

set -e  # 遇到错误时退出

echo "=================================="
echo "ASC MCP Server 一键运行脚本"
echo "=================================="

# 检查是否安装了 cargo
if ! command -v cargo &> /dev/null; then
    echo "错误: 未找到 cargo。请先安装 Rust 工具链。"
    echo "安装命令: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# 检查 Cargo.toml 文件是否存在
if [ ! -f "Cargo.toml" ]; then
    echo "错误: 未找到 Cargo.toml 文件。请确保在项目根目录下运行此脚本。"
    exit 1
fi

echo "开始构建 ASC MCP Server..."

# 构建项目
cargo build --release

echo "构建完成！"

echo "启动 ASC MCP Server..."

# 运行服务器
cargo run --release

echo "服务器已停止运行。"