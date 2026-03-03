@echo off
REM asc_mcp_server 一键运行脚本 (Windows版)
REM 作者: dpiner
REM 日期: %date%

echo ==================================
echo ASC MCP Server 一键运行脚本 (Windows版)
echo ==================================

REM 检查是否安装了 cargo
cargo --version >nul 2>&1
if errorlevel 1 (
    echo 错误: 未找到 cargo。请先安装 Rust 工具链。
    echo 安装命令: curl --proto ^=https --tlsv1.2 -sSf https://sh.rustup.rs ^| sh
    pause
    exit /b 1
)

REM 检查 Cargo.toml 文件是否存在
if not exist "Cargo.toml" (
    echo 错误: 未找到 Cargo.toml 文件。请确保在项目根目录下运行此脚本。
    pause
    exit /b 1
)

echo 开始构建 ASC MCP Server...
REM 构建项目
cargo build --release
if errorlevel 1 (
    echo 构建失败！
    pause
    exit /b 1
)

echo 构建完成！

echo 启动 ASC MCP Server...
REM 运行服务器
cargo run --release

echo 服务器已停止运行。
pause