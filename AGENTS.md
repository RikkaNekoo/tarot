# AGENTS.md

本文件为 AI 代理与开发者提供本项目的工作指南。

## 项目概述

`tarot` 是一个基于 PC/SC 架构的智能卡读取工具，采用前后端分离：

- 后端（`crates/backend`）：通过 `pcsc` 与读卡器交互，
  负责原生 APDU 交互并抓取原始字节（Raw Bytes）。普通卡不做业务解析；
  旅行证件因加密会话有状态，例外地在后端完成认证与解密（见下）。
- 前端（`crates/tui`）：基于 `ratatui` + `crossterm` 的 TUI，调用后端接口获取
  原始数据后在前端解析（TLV / PBOC 余额与交易 / FeliCa / MRZ 等），并展示 APDU 追踪。
- 共享层（`crates/core`）：APDU、响应、卡片数据模型、错误类型，
  以及旅行证件的输入参数类型 `PassportKey`。

## 目录结构

```
tarot/
├── AGENTS.md
├── Cargo.toml                    # workspace 根
├── docs/
│   ├── nfsee-apdu-analysis.md    # APDU 深度分析
│   ├── traveldoc-reading.md      # 旅行证件读取协议与字节布局
│   └── tui-prompt.md             # TUI 设计说明
└── crates/
    ├── core/                     # 共享类型（含 PassportKey）
    ├── backend/                  # PC/SC 后端 + CLI
    └── tui/                      # ratatui 前端
```

主要模块：

- `crates/backend/src/cards/` —— 每类卡一个模块，探测链集中在 `mod.rs`。
  - `traveldoc/` —— 旅行证件：`crypto`（BAC 密码学）、`sm`（安全通道）、
    `read`（SELECT → BAC → 读 DG 编排）。
- `crates/tui/src/parse/` —— 前端语义解析。
  - `traveldoc/` —— `mrz`（MRZ 原语）、`passport`（TD3 布局）、
    `eep`（往来港澳通行证 TD1 变体布局）。

## 硬件与协议注意事项

- 目标读卡器：PC/SC CCID 读卡器。
- ISO 14443-4（Type A/B）卡：APDU 直接透传。
- Mifare Classic/Ultralight、FeliCa：需走伪 APDU / 直传通道，
  与手机 NFC 原生层的封装不同。
- 卡片协议层用 ATR 解析 + Select 探测判定，不依赖上层给出的标准标记。
- 旅行证件读取需要 MRZ 三要素（证件号、出生日期、有效期）派生密钥，
  无法从芯片直接读出，必须由用户输入。

## 开发约定

- 普通卡：后端只返回 raw bytes，业务解析全部在前端。
- 旅行证件：BAC 与安全消息通道有状态（会话密钥 + 递增 SSC），
  必须在后端即时加解密，把解密后的 DG 明文存入 `raw_fields`，
  前端只解析明文语义（MRZ 字段、人脸图像等）。
- 每类卡一个 module，探测逻辑集中在 `cards/mod.rs` 的探测链。
- 旅行证件不入自动探测链（需外部密钥），由上层显式调用 `read_traveldoc`。
- 错误统一走 `core::Error`，覆盖读卡器断开、无卡、APDU 错误码、
  以及旅行证件专用的 `Error::Passport`。
- 提交前运行 `cargo build` 与 `cargo test`。

## 常用命令

```bash
cargo build                          # 构建全部
cargo test                           # 运行测试

# CLI 读卡
cargo run -p backend --bin cli -- list           # 列出读卡器
cargo run -p backend --bin cli -- read           # 读取一次
cargo run -p backend --bin cli -- monitor        # 插卡即读

# 读取旅行证件（护照 / 往来港澳通行证）
cargo run -p backend --bin cli -- traveldoc \
  --doc-number <证件号> --dob <YYMMDD> --doe <YYMMDD>

cargo run -p tui                     # 启动 TUI
```
