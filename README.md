# ASC MCP Server

ASC MCP Server 是一个用于处理PADS Logic原理图文件的Model Context Protocol (MCP)服务器。该服务器能够解析、修改和保存PADS Logic格式的原理图文件，支持器件更新、位置调整、网络连接管理等功能。

**注意**：此服务器需要与前端检查器配合使用才能正常运行。请确保已安装并运行 [asc_mcp_cheker](https://github.com/dpineer/asc_mcp_cheker) 前端检查器。

- **文件解析**：支持解析PADS Logic V3.0格式的原理图文件（.txt）和包含原理图的ZIP文件
- **器件管理**：更新器件ID、器件类型、封装等信息
- **位置调整**：修改器件在原理图中的坐标位置
- **网络管理**：清空所有网络连接、向特定网络添加引脚
- **文件保存**：将修改后的内容保存回原始文件或ZIP包
- **编码支持**：支持GBK编码的文件读写

## 技术栈

- **语言**：Rust
- **协议**：Model Context Protocol (MCP)
- **网络**：WebSocket
- **文件格式**：PADS Logic V3.0
- **压缩格式**：ZIP

## 安装要求

- Rust 1.70 或更高版本
- Cargo 包管理器

## 安装与运行

1. 克隆项目：
   ```bash
   git clone <repository-url>
   cd asc_mcp_server
   ```

2. 构建项目：
   ```bash
   cargo build --release
   ```

3. 运行服务器：
   ```bash
   cargo run
   ```
   
   或者直接运行：
   ```bash
   ./run_server.sh  # Linux/Mac
   ./run_server.bat # Windows
   ```

服务器默认监听 `ws://127.0.0.1:8080`。

## 支持的工具

### `get_full_data`
获取原理图文件的完整数据，包括器件信息、网络连接和坐标。

**参数**：
- `file_path`: 原理图文件路径（支持.txt和.zip文件）

**返回**：
- `parts`: 器件信息（ID、器件类型、封装、坐标）
- `nets`: 网络连接信息
- `lines`: 图形线段坐标
- `actual_path`: 实际处理的文件路径

### `update_component`
更新器件信息。

**参数**：
- `file_path`: 原理图文件路径
- `old_id`: 原器件ID
- `new_id`: 新器件ID
- `new_device`: 新器件类型

### `update_position`
更新器件位置。

**参数**：
- `file_path`: 原理图文件路径
- `component_id`: 器件ID
- `new_x`: 新的X坐标
- `new_y`: 新的Y坐标

### `save_file`
将修改后的内容保存回原始文件。

**参数**：
- `original_path`: 原始文件路径
- `modified_txt_path`: 修改后的TXT文件路径

### `clear_all_nets`
清空所有网络连接。

**参数**：
- `file_path`: 原理图文件路径

### `add_net_pin`
向特定网络添加引脚。

**参数**：
- `file_path`: 原理图文件路径
- `net_name`: 网络名称
- `pin`: 要添加的引脚

## 文件格式说明

服务器支持PADS Logic V3.0格式的原理图文件，该格式包含以下部分：

- `*PART*`: 器件定义部分
- `*NET*`: 网络连接部分
- `*SCH*`: 原理图布局部分（包含器件位置信息）
- `*LINES*`: 图形线段部分

## 使用场景

- 与AI助手集成，实现原理图的智能编辑
- 自动化原理图修改和优化
- 原理图文件格式转换
- 电子设计自动化(EDA)工具集成

## 错误处理

服务器会对文件操作进行错误处理，包括：
- 文件读取错误
- ZIP文件解析错误
- 文件格式不匹配
- 权限不足等

错误信息会通过MCP协议返回给客户端。

## 许可证

CC BY NC SA