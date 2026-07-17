# tarot

基于 PC/SC 的智能卡读取工具。用读卡器抓取卡片原始字节，在终端界面解析并展示余额、交易记录、证件信息与完整 APDU 追踪。  

采用前后端分离：后端只负责原生 APDU 交互与抓取原始字节，业务解析全部在前端完成。  
旅行证件（电子护照 / 电子往来港澳通行证）因加密会话有状态，例外地在后端完成 BAC 认证与安全通道解密。  
旅行证件的 MRZ 密钥与解密后的个人信息仅在本地内存处理，不做任何持久化。

## 支持的卡片

- 交通卡：深圳通、武汉通、岭南通、交通联合（TUnion）、北京一卡通、澳门通（Macau Pass）、TMoney、八达通（FeliCa）
- 银行卡：EMV 非接（银联 / Visa / MasterCard / AMEX / JCB / Discover）
- 芯片卡：Mifare Classic / Ultralight、DESFire
- 旅行证件：电子护照、电子往来港澳通行证（ICAO Doc 9303 / BAC）

## 环境要求

- Rust
- PC/SC 服务
- CCID 读卡器：PC/SC CCID 读卡器，在 ACS ACR1251U / Sony PaSoRi RC-S300 / HID OMNIKEY 5022（FeliCa 读取不可用）测试可用

## 快速开始

```bash
# 构建
cargo build --release

# 启动 TUI（推荐）
cargo run -p tarot-tui

# 或使用命令行
cargo run -p tarot-backend --bin cli -- read
```

## 用法

### TUI

```bash
cargo run -p tarot-tui
```

放上卡片即自动识别并展示解析结果与 APDU 追踪。读取旅行证件时需先输入 MRZ 三要素（证件号、出生日期、有效期）。  
持久化配置文件与交通卡历史记录分别保存在 `$HOME/.config/tarot/tui-settings.conf` 与 `$HOME/.local/share/tarot/tui-history.tsv`  
当 `$HOME` 不存在时则保存在运行目录下

### CLI

```bash
cli list                              # 列出读卡器
cli read [--reader NAME]              # 读取一次并打印原始数据
cli monitor [--reader NAME]           # 持续监控，插卡即读

# 读取旅行证件（护照 / 往来港澳通行证）
cli traveldoc --doc-number <证件号> --dob <YYMMDD> --doe <YYMMDD> [--reader NAME]
```

旅行证件的 MRZ 三要素无法从芯片读出，必须由持证人提供，用于派生 BAC 密钥。

## 项目结构

```
tarot/
├── crates/
│   ├── core/       # 共享类型：APDU、卡片数据模型、错误、PassportKey
│   ├── backend/    # PC/SC 后端 + CLI（tarot-backend / cli）
│   └── tui/        # ratatui 终端界面（tarot-tui / tui）
├── docs/
│   ├── apdu-analysis.md        # APDU 深度分析
│   ├── rc-s300.md              # Sony RC-S300 PC/SC 指令与通信方式
│   └── traveldoc-reading.md    # 旅行证件读取协议与字节布局
└── AGENTS.md       # 开发指南
```

## 开发

```bash
cargo build          # 构建全部
cargo test           # 运行测试
cargo clippy         # 静态检查
```

## 致谢

- [nfsee](https://github.com/nfcim/nfsee)：本项目的基础，大多数 APDU 指令均来自 nfsee，节省了大量精力。
- [T-Union_Master](https://github.com/SocialSisterYi/T-Union_Master)：提供了交通联合行程记录读取的思路。
- [CoreExtendedNFC](https://github.com/Lakr233/CoreExtendedNFC)：提供了旅行证件读取的思路。
